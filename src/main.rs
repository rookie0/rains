use anyhow::Result;
use rains::{
    cli::{Opts, Subcommand},
    sina::Sina,
};
use regex::Regex;
use tracing::{debug, Level};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{}", err);
        std::process::exit(0x0100)
    }
}

async fn run() -> Result<()> {
    let args = Opts::parse_args()?;
    if args.debug {
        tracing_subscriber::fmt().with_max_level(Level::DEBUG).init();
    } else {
        tracing_subscriber::fmt().without_time().with_target(false).init();
    }
    debug!("args: {:?}", args);

    match args.cmd {
        Subcommand::Search { query, limit } => {
            let results = Sina::default().search(&query).await.unwrap();
            let limit = if (limit as usize) < results.len() { limit as usize } else { results.len() };
            for i in 0..limit {
                let invest = results.get(i).unwrap();
                println!("{} \t {}", invest.symbol, invest.name);
            }
        }
        Subcommand::Info { symbol, .. } => {
            match Regex::new(r"^(SZ|SH|BJ)\d{6}").unwrap().captures(&symbol.to_uppercase()) {
                Some(caps) if symbol.len() == 8 => {
                    let code = &caps.get(0).unwrap().as_str()[2..];
                    let profile = Sina::default().profile(code).await.unwrap();
                    println!("证券代码：\t{}\n简称历史：\t{}\n公司名称：\t{}\n主营业务：\t{}\n公司网址：\t{}\n办公地址：\t{}\n上市日期：\t{}\n发行价格：\t{}", symbol, profile.used_name, profile.name, profile.business, profile.website, profile.business_address, profile.listing_date, profile.listing_price);
                }
                _ => eprintln!("当前仅支持沪深及北证股票信息查询"),
            }
        }
    }

    Ok(())
}
