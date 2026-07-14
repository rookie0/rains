/// 行情报价
#[derive(Debug, Default, Clone)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    /// 当前价
    pub now: f64,
    /// 昨收
    pub close: f64,
    /// 今开
    pub open: f64,
    /// 最高
    pub high: f64,
    /// 最低
    pub low: f64,
    /// 竞买价
    pub buy: f64,
    /// 竞卖价
    pub sell: f64,
    /// 成交量（历史公开字段名保留为 turnover）
    pub turnover: f64,
    /// 成交额（历史公开字段名保留为 volume）
    pub volume: f64,
    pub date: String,
    pub time: String,
    // pub currency: String,
}

impl Quote {
    /// 语义明确的成交量访问器，隔离历史字段命名。
    pub fn traded_volume(&self) -> f64 {
        self.turnover
    }

    /// 语义明确的成交额访问器，隔离历史字段命名。
    pub fn traded_amount(&self) -> f64 {
        self.volume
    }
}
