use solana_sdk::signature::{Keypair, Signer};
use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

const MAX_LINES_PER_FILE: u64 = 1_000_000;

#[derive(Debug, Clone)]
enum LogMessage {
    Regular {
        time_str: String,
        counter: u64,
        public_key: String,
        private_key: String,
    },
    Found {
        time_str: String,
        counter: u64,
        public_key: String,
        private_key: String,
    },
}

struct LogWriter {
    writer: BufWriter<File>,
    file_index: u32,
    line_count: u64,
}

impl LogWriter {
    fn new() -> std::io::Result<Self> {
        let file_index = 0;
        let file_path = format!("keypairs_{:04}.log", file_index);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        let writer = BufWriter::new(file);
        
        println!("创建日志文件: {}", file_path);
        
        Ok(LogWriter {
            writer,
            file_index,
            line_count: 0,
        })
    }

    fn write_line(&mut self, content: &str) -> std::io::Result<()> {
        writeln!(self.writer, "{}", content)?;
        self.writer.flush()?;
        self.line_count += 1;

        // 如果达到最大行数，创建新文件
        if self.line_count >= MAX_LINES_PER_FILE {
            self.rotate_file()?;
        }

        Ok(())
    }

    fn rotate_file(&mut self) -> std::io::Result<()> {
        // 关闭当前文件（通过 flush 和 drop）
        self.writer.flush()?;
        
        // 创建新文件
        self.file_index += 1;
        self.line_count = 0;
        let file_path = format!("keypairs_{:04}.log", self.file_index);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        
        // 替换 writer（旧的 writer 会被自动 drop）
        self.writer = BufWriter::new(file);
        
        println!("创建新日志文件: {} (已达到 {} 行)", file_path, MAX_LINES_PER_FILE);
        
        Ok(())
    }

    fn finalize(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

fn main() -> std::io::Result<()> {
    let target_prefix = "seekr";
    
    // 获取CPU核心数，创建相应数量的工作线程
    let num_threads = num_cpus::get();
    println!("检测到 {} 个CPU核心，将使用 {} 个工作线程", num_threads, num_threads);
    
    println!("开始生成密钥对，寻找以 '{}' 开头的公钥地址...\n", target_prefix);
    println!("日志将保存到 keypairs_XXXX.log 文件中，每个文件最多 {} 行\n", MAX_LINES_PER_FILE);

    // 共享状态
    let counter = Arc::new(AtomicU64::new(0));
    let found = Arc::new(AtomicBool::new(false));
    let found_result = Arc::new(Mutex::new(None::<(u64, String, String, String)>));
    
    // Channel用于将日志消息发送到写入线程
    let (log_tx, log_rx) = mpsc::channel::<LogMessage>();
    
    // 启动日志写入线程
    let log_writer_handle = {
        let log_rx = log_rx;
        thread::spawn(move || -> std::io::Result<()> {
            let mut log_writer = LogWriter::new()?;
            
            loop {
                match log_rx.recv() {
                    Ok(LogMessage::Regular { time_str, counter, public_key, private_key }) => {
                        let log_line = format!(
                            "[{}] 序号: {} | 公钥: {} | 私钥: {}",
                            time_str, counter, public_key, private_key
                        );
                        log_writer.write_line(&log_line)?;
                    }
                    Ok(LogMessage::Found { time_str, counter, public_key, private_key }) => {
                        let log_line = format!(
                            "[{}] [FOUND] 序号: {} | 公钥: {} | 私钥: {}",
                            time_str, counter, public_key, private_key
                        );
                        log_writer.write_line(&log_line)?;
                        log_writer.finalize()?;
                        break;
                    }
                    Err(_) => {
                        // Channel关闭，所有发送者都已退出
                        log_writer.finalize()?;
                        break;
                    }
                }
            }
            Ok(())
        })
    };
    
    // 启动工作线程
    let mut handles = Vec::new();
    for thread_id in 0..num_threads {
        let counter = Arc::clone(&counter);
        let found = Arc::clone(&found);
        let found_result = Arc::clone(&found_result);
        let log_tx = log_tx.clone();
        let target_prefix = target_prefix.to_string();
        
        let handle = thread::spawn(move || {
            let mut local_counter = 0u64;
            
            loop {
                // 检查是否已经找到目标
                if found.load(Ordering::Relaxed) {
                    break;
                }
                
                // 生成新的密钥对
                let keypair = Keypair::new();
                let public_key = keypair.pubkey();
                let public_key_str = public_key.to_string();
                let private_key_str = bs58::encode(keypair.to_bytes()).into_string();
                
                // 获取当前时间
                let now = Local::now();
                let millis = now.timestamp_millis() % 1000;
                let time_str = format!("{}-{:03}", now.format("%Y%m%d%H%M%S"), millis);
                
                // 原子递增计数器
                let global_counter = counter.fetch_add(1, Ordering::Relaxed) + 1;
                local_counter += 1;
                
                // 检查公钥是否以目标前缀开头
                if public_key_str.starts_with(&target_prefix) {
                    // 尝试设置found标志（只有第一个找到的线程会成功）
                    if !found.swap(true, Ordering::Relaxed) {
                        // 保存找到的结果
                        let result_time_str = time_str.clone();
                        let result_public_key = public_key_str.clone();
                        let result_private_key = private_key_str.clone();
                        
                        *found_result.lock().unwrap() = Some((
                            global_counter,
                            result_time_str.clone(),
                            result_public_key.clone(),
                            result_private_key.clone(),
                        ));
                        
                        // 发送找到的消息
                        let _ = log_tx.send(LogMessage::Found {
                            time_str: result_time_str.clone(),
                            counter: global_counter,
                            public_key: result_public_key.clone(),
                            private_key: result_private_key.clone(),
                        });
                        
                        println!(
                            "[线程 {}] ✓ 找到目标地址！\n序号: {}\n时间: {}\n公钥: {}\n私钥: {}",
                            thread_id, global_counter, result_time_str, result_public_key, result_private_key
                        );
                    }
                    break;
                } else {
                    // 发送常规日志消息
                    let _ = log_tx.send(LogMessage::Regular {
                        time_str,
                        counter: global_counter,
                        public_key: public_key_str,
                        private_key: private_key_str,
                    });
                    
                    // 控制台输出简化版本（每1000条输出一次，避免刷屏）
                    if global_counter % 1000 == 0 {
                        println!("[线程 {}] 已生成 {} 条记录 (本线程生成了 {} 条)", 
                                thread_id, global_counter, local_counter);
                    }
                }
            }
        });
        
        handles.push(handle);
    }
    
    // 等待所有工作线程完成
    for handle in handles {
        handle.join().unwrap();
    }
    
    // 关闭channel，通知日志写入线程退出
    drop(log_tx);
    
    // 等待日志写入线程完成
    log_writer_handle.join().unwrap()?;
    
    // 显示找到的结果
    if let Some((counter, time_str, public_key, private_key)) = found_result.lock().unwrap().take() {
        let success_msg = format!(
            "\n✓ 找到目标地址！\n序号: {}\n时间: {}\n公钥: {}\n私钥: {}",
            counter, time_str, public_key, private_key
        );
        println!("{}", success_msg);
    }
    
    println!("\n程序完成，日志已保存。");
    
    Ok(())
}

