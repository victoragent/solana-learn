use solana_sdk::signature::{Keypair, Signer};
use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};

const MAX_LINES_PER_FILE: u64 = 1_000_000;

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

    fn finalize(mut self) -> std::io::Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

fn main() -> std::io::Result<()> {
    let target_prefix = "seekr";
    let mut counter = 0u64;

    println!("开始生成密钥对，寻找以 '{}' 开头的公钥地址...\n", target_prefix);
    println!("日志将保存到 keypairs_XXXX.log 文件中，每个文件最多 {} 行\n", MAX_LINES_PER_FILE);

    let mut log_writer = LogWriter::new()?;

    loop {
        counter += 1;
        
        // 生成新的密钥对
        let keypair = Keypair::new();
        let public_key = keypair.pubkey();
        let public_key_str = public_key.to_string();
        let private_key_str = bs58::encode(keypair.to_bytes()).into_string();

        // 获取当前时间
        let now = Local::now();
        let millis = now.timestamp_millis() % 1000;
        let time_str = format!("{}-{:03}", now.format("%Y%m%d%H%M%S"), millis);

        // 检查公钥是否以目标前缀开头
        if public_key_str.starts_with(target_prefix) {
            let success_msg = format!(
                "✓ 找到目标地址！\n序号: {}\n时间: {}\n公钥: {}\n私钥: {}",
                counter, time_str, public_key_str, private_key_str
            );
            println!("{}", success_msg);
            log_writer.write_line(&format!(
                "[{}] [FOUND] 序号: {} | 公钥: {} | 私钥: {}",
                time_str, counter, public_key_str, private_key_str
            ))?;
            break;
        } else {
            // 写入日志：时间、序号、公钥、私钥
            let log_line = format!(
                "[{}] 序号: {} | 公钥: {} | 私钥: {}",
                time_str, counter, public_key_str, private_key_str
            );
            log_writer.write_line(&log_line)?;
            
            // 控制台输出简化版本（每1000条输出一次，避免刷屏）
            if counter % 1000 == 0 {
                println!("[{}] 已生成 {} 条记录，当前公钥: {}", time_str, counter, public_key_str);
            }
        }
    }

    log_writer.finalize()?;
    println!("\n程序完成，日志已保存。");
    
    Ok(())
}

