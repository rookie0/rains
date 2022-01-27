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
    pub listing_price: String,
    /// 上市日期
    pub listing_date: String,
    /// 行业分类
    // pub category: String,
    /// 主营业务
    pub business: String,
    /// 办公地址
    pub business_address: String,
    /// 公司网址
    pub website: String,
}

#[derive(Debug, Default)]
pub struct Financial {
    /// 财报日期
    pub report_date: String,
    /// 财报名称
    pub report_name: String,
    /// 总营收
    pub total_revenue: String,
    /// 营收同比增长
    pub total_revenue_rate: String,
    /// 净利润
    pub net_profit: String,
    /// 净利润同比增长
    pub net_profit_rate: String,
    /// 扣非净利润
    pub net_profit_after_nrgal: String,
    /// 扣非净利润同比增长
    pub net_profit_after_nrgal_rate: String,
    /// 每股收益
    pub eps: String,
    /// 每股未分配利润
    pub eps_undistributed: String,
    /// 每股净资产
    pub ps_net_assets: String,
    /// 每股资本公积金
    pub ps_capital_reserve: String,
    /// 每股经营现金流
    pub ps_cash_flow: String,
    /// 净资产收益率
    pub roe: String,
}

#[derive(Debug, Default)]
pub struct Structure {}

#[derive(Debug, Default)]
pub struct Dividend {}

#[derive(Debug, Default)]
pub struct Press {}
