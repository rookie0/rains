use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[clap(about, version)]
pub struct Opts {
    #[clap(short, long)]
    pub debug: bool,

    #[clap(subcommand)]
    pub cmd: Subcommand,
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum Subcommand {
    /// 搜索股票
    #[clap(alias = "s")]
    Search {
        /// 拼音/代码/名称 eg: zgpa
        #[clap(required = true)]
        query: String,
        /// 展示条数
        #[clap(short, long, default_value_t = 10)]
        limit: u8,
    },
    /// 股票信息
    #[clap(alias = "i")]
    Info {
        /// 证券代码 eg: SH601318
        #[clap(required = true)]
        symbol: String,
        /// 全部信息
        #[clap(short, long)]
        all: bool,
        /// 财务指标
        #[clap(short, long)]
        financials: bool,
        /// 股东结构
        #[clap(short, long)]
        structure: bool,
        /// 分红送配
        #[clap(short, long)]
        dividends: bool,
        /// 最新公告
        #[clap(short, long)]
        presses: bool,
    },
    /// 行情报价
    #[clap(alias = "q")]
    Quote {
        /// 证券代码 多个以 , 分隔 eg: SH601318,SZ000001
        #[clap(required = true)]
        symbol: String,
        /// 不检测代码是否正确
        #[clap(short, long)]
        no_check: bool,
        /// 实时行情
        #[clap(short, long)]
        realtime: bool,
        /// 实时行情多行展示 仅单个时支持
        #[clap(short, long)]
        multiline: bool,
    },
}

impl Opts {
    pub fn parse_args() -> Result<Self> {
        let opts = Self::parse();
        Ok(opts)
    }
}
