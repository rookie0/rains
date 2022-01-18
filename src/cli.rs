use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[clap(about, version)]
pub struct Opts {
    #[clap(subcommand)]
    pub cmd: Subcommand,
}

#[derive(Debug, Eq, PartialEq, Parser)]
pub enum Subcommand {
    /// 搜索
    #[clap(alias = "s")]
    Search {
        #[clap(required = true)]
        query: String,
    },
    /// 股票信息
    #[clap(alias = "i")]
    Info {
        #[clap(required = true)]
        stock: String,
    },
}

impl Opts {
    pub fn parse_args() -> Result<Self> {
        let opts = Self::parse();
        Ok(opts)
    }
}
