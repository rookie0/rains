/// 股票信息
#[derive(Debug, Default)]
pub struct Stock {
    /// 代码
    pub symbol: String,
    /// 简称
    pub name: String,
    /// 公司信息
    pub profile: Profile,
    /// 财务指标
    pub financials: Vec<Financial>,
    /// 股东结构
    pub structure: Vec<Structure>,
    /// 分红送配
    pub dividends: Vec<Dividend>,
    /// 最新公告
    pub presses: Vec<Press>,
}

#[derive(Debug, Default)]
pub struct Profile {
    /// 公司名称
    pub name: String,
    /// 简称历史
    pub used_name: String,
    /// 上市价格
    pub listing_price: f64,
    /// 上市日期
    pub listing_date: String,
    /// 行业分类
    pub category: String,
    /// 主营业务
    pub business: String,
    /// 办公地址
    pub business_address: String,
    /// 公司网址
    pub website: String,
    /// 当前价格
    pub price: f64,
    /// 市净率
    pub pb: f64,
    /// 市盈率TTM
    pub pe_ttm: f64,
    /// 总市值
    pub market_cap: f64,
    /// 流通市值
    pub traded_market_cap: f64,
}

#[derive(Debug, Default, Clone)]
pub struct Financial {
    /// 财报日期
    pub date: String,
    /// 总营收
    pub total_revenue: f64,
    /// 营收同比增长
    pub total_revenue_rate: f64,
    /// 净利润
    pub net_profit: f64,
    /// 净利润同比增长
    pub net_profit_rate: f64,
    /// 扣非净利润
    pub net_profit_after_nrgal: f64,
    /// 扣非净利润同比增长
    pub net_profit_after_nrgal_rate: f64,
    /// 每股收益
    pub eps: f64,
    /// 每股未分配利润
    pub eps_undistributed: f64,
    /// 每股净资产
    pub ps_net_assets: f64,
    /// 每股资本公积金
    pub ps_capital_reserve: f64,
    /// 每股经营现金流
    pub ps_cash_flow: f64,
    /// 净资产收益率
    pub roe: f64,
}

#[derive(Debug, Default)]
pub struct Structure {
    pub date: String,
    /// 股东总数
    pub holders_num: f64,
    /// 平均持股数
    pub shares_avg: f64,
    /// 十大股东
    pub holders_ten: Vec<Holder>,
}

#[derive(Debug, Default)]
pub struct Holder {
    pub name: String,
    pub shares: f64,
    pub percent: f64,
    pub shares_type: String,
}

#[derive(Debug, Default)]
pub struct Dividend {
    /// 公告日
    pub date: String,
    /// 登记日
    pub date_record: String,
    /// 除息日
    pub date_dividend: String,
    /// 送股
    pub shares_dividend: f64,
    /// 转增股
    pub shares_into: f64,
    /// 红利
    pub money: f64,
}

#[derive(Debug, Default)]
pub struct Press {
    pub date: String,
    pub title: String,
    pub url: String,
    pub file: String,
}
