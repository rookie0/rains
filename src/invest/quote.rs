/// 行情报价
#[derive(Debug, Default)]
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
    /// 成交量
    pub turnover: f64,
    /// 成交额
    pub volume: f64,
    pub date: String,
    pub time: String,
    // pub currency: String,
}
