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
    /// 搜索 拼音/代码/名称 eg: zgpa
    #[clap(alias = "s")]
    Search {
        #[clap(required = true)]
        query: String,
        /// 条数 默认10条
        #[clap(short, long, default_value_t = 10)]
        limit: u8,
    },
    /// 公司信息 eg: SH601318
    #[clap(alias = "i")]
    Info {
        #[clap(required = true)]
        symbol: String,
        #[clap(short, long)]
        all: bool,
        #[clap(short, long)]
        financials: bool,
        #[clap(short, long)]
        structure: bool,
        #[clap(short, long)]
        dividends: bool,
        #[clap(short, long)]
        presses: bool,
    },
}

impl Opts {
    pub fn parse_args() -> Result<Self> {
        let opts = Self::parse();
        Ok(opts)
    }
}
