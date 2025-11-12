use solana_sdk::signature::{Keypair, Signer};
use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::env;
use std::collections::HashSet;

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
        matched_prefix: String,
    },
}

struct LogWriter {
    writer: BufWriter<File>,
    file_index: u32,
    line_count: u64,
}

struct ResultWriter {
    writer: BufWriter<File>,
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

impl ResultWriter {
    fn new() -> std::io::Result<Self> {
        let file_path = "result.log";
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        let writer = BufWriter::new(file);
        
        println!("创建结果文件: {}", file_path);
        
        Ok(ResultWriter { writer })
    }

    fn write_result(&mut self, time_str: &str, counter: u64, public_key: &str, private_key: &str, matched_prefix: &str) -> std::io::Result<()> {
        let log_line = format!(
            "[{}] [FOUND] 匹配前缀: {} | 序号: {} | 公钥: {} | 私钥: {}",
            time_str, matched_prefix, counter, public_key, private_key
        );
        writeln!(self.writer, "{}", log_line)?;
        self.writer.flush()?;
        Ok(())
    }

    fn finalize(&mut self) -> std::io::Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

#[derive(Debug)]
struct Config {
    num_threads: Option<usize>,
    prefixes: Vec<String>,
}

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = env::args().collect();
    let mut num_threads = None;
    let mut prefixes = Vec::new();
    
    let mut i = 1; // 跳过程序名
    while i < args.len() {
        if args[i] == "--threads" || args[i] == "-t" {
            if i + 1 < args.len() {
                match args[i + 1].parse::<usize>() {
                    Ok(num) => {
                        num_threads = Some(num);
                        i += 2;
                    }
                    Err(_) => {
                        return Err(format!("错误: '{}' 不是有效的线程数", args[i + 1]));
                    }
                }
            } else {
                return Err(format!("错误: {} 参数需要指定线程数", args[i]));
            }
        } else if args[i] == "--prefix" || args[i] == "-p" {
            // 支持多个前缀，可以多次使用 --prefix 或一次指定多个
            if i + 1 < args.len() {
                // 检查下一个参数是否也是选项
                if !args[i + 1].starts_with('-') {
                    prefixes.push(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err(format!("错误: {} 参数需要指定至少一个前缀", args[i]));
                }
            } else {
                return Err(format!("错误: {} 参数需要指定至少一个前缀", args[i]));
            }
        } else if args[i].starts_with('-') {
            return Err(format!("错误: 未知参数 '{}'", args[i]));
        } else {
            // 如果没有指定 --prefix，但提供了非选项参数，也作为前缀处理
            prefixes.push(args[i].clone());
            i += 1;
        }
    }
    
    Ok(Config { num_threads, prefixes })
}

fn print_usage() {
    println!("用法:");
    println!("  cargo run [--release] -- [选项] [前缀1] [前缀2] ...");
    println!();
    println!("选项:");
    println!("  --threads, -t <数量>    指定使用的工作线程数（默认为CPU核心数）");
    println!("  --prefix, -p <前缀>     指定要搜索的公钥前缀（可多次使用指定多个前缀）");
    println!();
    println!("说明:");
    println!("  可以多次使用 --prefix 指定多个前缀，也可以直接提供前缀作为位置参数");
    println!("  程序会持续运行直到所有指定的前缀都被找到");
    println!("  找到的结果会保存到 result.log 文件中");
    println!();
    println!("示例:");
    println!("  cargo run -- --threads 8 --prefix seekr");
    println!("  cargo run -- --prefix seekr --prefix solana");
    println!("  cargo run -- seekr solana");
    println!("  cargo run --release -- -t 16 -p seekr -p test");
}

fn main() -> std::io::Result<()> {
    // 检查是否有 --help 或 -h
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }
    
    // 解析命令行参数
    let config = match parse_args() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("{}", err);
            print_usage();
            std::process::exit(1);
        }
    };
    
    // 处理线程数
    let num_threads = match config.num_threads {
        Some(num) => {
            if num == 0 {
                eprintln!("错误: 线程数必须大于 0");
                print_usage();
                std::process::exit(1);
            }
            let max_cores = num_cpus::get();
            if num > max_cores * 2 {
                eprintln!("警告: 指定的线程数 {} 超过建议值（CPU核心数的2倍: {}），可能会影响性能", num, max_cores * 2);
            }
            num
        }
        None => {
            // 默认使用CPU核心数
            num_cpus::get()
        }
    };
    
    // 处理前缀
    let target_prefixes: Vec<String> = if config.prefixes.is_empty() {
        // 如果没有指定前缀，使用默认值
        vec!["seekr".to_string()]
    } else {
        config.prefixes
    };
    
    let max_cores = num_cpus::get();
    
    if num_threads == max_cores {
        println!("检测到 {} 个CPU核心，将使用 {} 个工作线程（默认）", max_cores, num_threads);
    } else {
        println!("检测到 {} 个CPU核心，将使用 {} 个工作线程（用户指定）", max_cores, num_threads);
    }
    
    println!("目标前缀: {:?}", target_prefixes);
    println!("开始生成密钥对，寻找以这些前缀开头的公钥地址...");
    println!("程序将持续运行直到所有前缀都被找到\n");
    println!("日志将保存到 keypairs_XXXX.log 文件中，每个文件最多 {} 行", MAX_LINES_PER_FILE);
    println!("找到的结果将保存到 result.log 文件中\n");

    // 共享状态
    let counter = Arc::new(AtomicU64::new(0));
    let found_prefixes = Arc::new(Mutex::new(HashSet::<String>::new()));
    let all_found = Arc::new(AtomicBool::new(false));
    
    // 使用两个独立的 channel：一个用于常规日志，一个用于结果
    let (regular_log_tx, regular_log_rx) = mpsc::channel::<LogMessage>();
    let (result_tx, result_rx) = mpsc::channel::<LogMessage>();
    
    // 启动日志写入线程（常规日志）
    let log_writer_handle = {
        let regular_log_rx = regular_log_rx;
        thread::spawn(move || -> std::io::Result<()> {
            let mut log_writer = LogWriter::new()?;
            
            loop {
                match regular_log_rx.recv() {
                    Ok(LogMessage::Regular { time_str, counter, public_key, private_key }) => {
                        let log_line = format!(
                            "[{}] 序号: {} | 公钥: {} | 私钥: {}",
                            time_str, counter, public_key, private_key
                        );
                        log_writer.write_line(&log_line)?;
                    }
                    Ok(LogMessage::Found { .. }) => {
                        // Found 消息由结果写入线程处理，这里只处理常规日志
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
    
    // 启动结果写入线程（result.log）
    let result_writer_handle = {
        let result_rx = result_rx;
        let found_prefixes = Arc::clone(&found_prefixes);
        let all_found = Arc::clone(&all_found);
        let target_prefixes = target_prefixes.clone();
        thread::spawn(move || -> std::io::Result<()> {
            let mut result_writer = ResultWriter::new()?;
            
            loop {
                match result_rx.recv() {
                    Ok(LogMessage::Found { time_str, counter, public_key, private_key, matched_prefix }) => {
                        // 检查这个前缀是否已经被记录过
                        let mut found_set = found_prefixes.lock().unwrap();
                        if !found_set.contains(&matched_prefix) {
                            found_set.insert(matched_prefix.clone());
                            
                            // 写入结果文件
                            result_writer.write_result(
                                &time_str,
                                counter,
                                &public_key,
                                &private_key,
                                &matched_prefix
                            )?;
                            
                            println!(
                                "✓ 找到匹配前缀 '{}' 的地址！\n序号: {}\n时间: {}\n公钥: {}\n私钥: {}\n",
                                matched_prefix, counter, time_str, public_key, private_key
                            );
                            
                            // 检查是否所有前缀都已找到
                            if found_set.len() >= target_prefixes.len() {
                                println!("🎉 所有目标前缀都已找到！");
                                all_found.store(true, Ordering::Relaxed);
                                result_writer.finalize()?;
                                break;
                            } else {
                                let remaining: Vec<_> = target_prefixes.iter()
                                    .filter(|p| !found_set.contains(*p))
                                    .collect();
                                println!("剩余目标: {:?}\n", remaining);
                            }
                        }
                    }
                    Ok(LogMessage::Regular { .. }) => {
                        // 结果 channel 不应该收到常规日志
                    }
                    Err(_) => {
                        // Channel关闭
                        result_writer.finalize()?;
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
        let all_found = Arc::clone(&all_found);
        let regular_log_tx = regular_log_tx.clone();
        let result_tx = result_tx.clone();
        let target_prefixes = target_prefixes.clone();
        
        let handle = thread::spawn(move || {
            let mut local_counter = 0u64;
            
            loop {
                // 检查是否所有目标都已找到
                if all_found.load(Ordering::Relaxed) {
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
                
                // 检查公钥是否匹配任何一个目标前缀
                let mut matched = false;
                for prefix in &target_prefixes {
                    if public_key_str.starts_with(prefix) {
                        matched = true;
                        // 发送找到的消息到结果 channel
                        let _ = result_tx.send(LogMessage::Found {
                            time_str: time_str.clone(),
                            counter: global_counter,
                            public_key: public_key_str.clone(),
                            private_key: private_key_str.clone(),
                            matched_prefix: prefix.clone(),
                        });
                        break;
                    }
                }
                
                if !matched {
                    // 发送常规日志消息
                    let _ = regular_log_tx.send(LogMessage::Regular {
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
    drop(regular_log_tx);
    drop(result_tx);
    
    // 等待日志写入线程完成
    log_writer_handle.join().unwrap()?;
    
    // 等待结果写入线程完成
    result_writer_handle.join().unwrap()?;
    
    // 显示找到的所有结果
    let found_set = found_prefixes.lock().unwrap();
    println!("\n程序完成！");
    println!("找到的前缀: {:?}", found_set);
    println!("日志已保存到 keypairs_XXXX.log");
    println!("结果已保存到 result.log");
    
    Ok(())
}

