//! 数据提供者模块
//!
//! 定义数据源抽象接口，支持多种数据源实现

mod sina;

use anyhow::Result;
use async_trait::async_trait;
pub use sina::SinaProvider;

/// 单次行情请求的唯一证券代码上限；命令层与 provider 边界共用同一策略。
pub const MAX_QUOTE_SYMBOLS: usize = 64;

use crate::invest::{
    Investment,
    quote::Quote,
    stock::{Dividend, Financial, Press, Profile, Structure},
};

/// 数据提供者 trait
#[async_trait]
pub trait Provider: Send + Sync {
    /// 搜索投资品
    async fn search(&self, query: &str) -> Result<Vec<Investment>>;

    /// 获取公司信息
    async fn profile(&self, symbol: &str) -> Result<Profile>;

    /// 获取财务指标
    async fn financials(&self, code: &str) -> Result<Vec<Financial>>;

    /// 获取股东结构
    async fn structures(&self, code: &str) -> Result<Vec<Structure>>;

    /// 获取分红送配
    async fn dividends(&self, code: &str) -> Result<Vec<Dividend>>;

    /// 获取最新公告
    async fn presses(&self, code: &str) -> Result<Vec<Press>>;

    /// 获取行情快照
    async fn quotes(&self, symbols: &str) -> Result<Vec<Quote>>;
}

/// 实时行情提供者 trait
#[async_trait]
pub trait RealtimeProvider: Send + Sync {
    /// 订阅实时行情
    async fn quotes_realtime<F>(&self, symbols: &str, handler: F) -> Result<()>
    where
        F: FnMut(Vec<Quote>) -> Result<()> + Send + 'static;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invest::{Exchange, Market};

    // ==================== 网络测试 ====================

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_search_a_share() {
        let provider = SinaProvider::default();
        let results = provider.search("zgpa").await.unwrap();

        assert!(!results.is_empty());
        let first = results.first().unwrap();
        assert!(first.name.contains("平安"));
        assert_eq!(first.market, Some(Market::Stock));
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_search_hk_share() {
        let provider = SinaProvider::default();
        let results = provider.search("00700").await.unwrap();

        assert!(!results.is_empty());
        let first = results.first().unwrap();
        assert!(first.name.contains("腾讯"));
        assert_eq!(first.exchange, Some(Exchange::HKex));
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_search_us_share() {
        let provider = SinaProvider::default();
        let results = provider.search("baba").await.unwrap();

        assert!(!results.is_empty());
        let alibaba = results.iter().find(|i| i.symbol == "$BABA");
        assert!(alibaba.is_some());
        assert_eq!(alibaba.unwrap().exchange, Some(Exchange::Nasdaq));
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_quotes() {
        let provider = SinaProvider::default();
        let quotes = provider.quotes("sh601318").await.unwrap();

        assert_eq!(quotes.len(), 1);
        let quote = quotes.first().unwrap();
        assert_eq!(quote.symbol, "SH601318");
        assert!(quote.now > 0.0);
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_quotes_multiple() {
        let provider = SinaProvider::default();
        let quotes = provider.quotes("sh601318,sz000001").await.unwrap();

        assert_eq!(quotes.len(), 2);
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_profile() {
        let provider = SinaProvider::default();
        let profile = provider.profile("SH601318").await.unwrap();

        assert!(profile.name.contains("平安"));
        assert!(profile.price > 0.0);
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_financials() {
        let provider = SinaProvider::default();
        let result = provider.financials("601318").await;

        // 财务数据查询应成功
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires the live Sina service"]
    async fn test_dividends() {
        let provider = SinaProvider::default();
        let dividends = provider.dividends("601318").await.unwrap();

        assert!(!dividends.is_empty());
    }
}
