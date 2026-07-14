//! 命令处理模块

mod info;
mod quote;
mod search;

use anyhow::{Result, bail};
pub use info::{InfoOptions, handle_info};
pub use quote::{QuoteOptions, handle_quote};
pub use search::handle_search;

use crate::{
    invest::{Exchange, Investment, Market},
    provider::Provider,
};

/// 验证股票代码有效性
pub async fn validate_symbol<P: Provider>(provider: &P, symbol: &str) -> Result<Investment> {
    let invest = symbol.parse::<Investment>()?;

    // 构建查询字符串
    let query = match invest.symbol.as_str() {
        s if s.starts_with("HK") => &s[2..],
        s if s.starts_with('$') => &s[1..],
        s => s,
    };

    let results = provider.search(query).await?;

    let exact = results
        .into_iter()
        .find(|candidate| candidate.symbol.eq_ignore_ascii_case(&invest.symbol))
        .ok_or_else(|| anyhow::anyhow!("代码错误，查无精确匹配结果"))?;

    if exact.market != Some(Market::Stock) {
        bail!("代码错误，当前只支持股票相关查询");
    }

    Ok(exact)
}

/// 检查是否为支持详情查询的交易所
pub fn is_supported_exchange(exchange: &Option<Exchange>) -> bool {
    matches!(exchange, Some(Exchange::Sse) | Some(Exchange::SZse) | Some(Exchange::Bse))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::{
        invest::{
            quote::Quote,
            stock::{Dividend, Financial, Press, Profile, Structure},
        },
        provider::RealtimeProvider,
    };

    #[derive(Default)]
    struct MockProvider {
        search_results: Vec<Investment>,
        search_queries: Mutex<Vec<String>>,
        quote_queries: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn search(&self, query: &str) -> Result<Vec<Investment>> {
            self.search_queries.lock().unwrap().push(query.to_string());
            Ok(self.search_results.clone())
        }

        async fn profile(&self, _symbol: &str) -> Result<Profile> {
            Ok(Profile::default())
        }

        async fn financials(&self, _code: &str) -> Result<Vec<Financial>> {
            Ok(Vec::new())
        }

        async fn structures(&self, _code: &str) -> Result<Vec<Structure>> {
            Ok(Vec::new())
        }

        async fn dividends(&self, _code: &str) -> Result<Vec<Dividend>> {
            Ok(Vec::new())
        }

        async fn presses(&self, _code: &str) -> Result<Vec<Press>> {
            Ok(Vec::new())
        }

        async fn quotes(&self, symbols: &str) -> Result<Vec<Quote>> {
            self.quote_queries.lock().unwrap().push(symbols.to_string());
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl RealtimeProvider for MockProvider {
        async fn quotes_realtime<F>(&self, _symbols: &str, mut handler: F) -> Result<()>
        where
            F: FnMut(Vec<Quote>) -> Result<()> + Send + 'static,
        {
            handler(Vec::new())
        }
    }

    fn stock(symbol: &str) -> Investment {
        Investment {
            symbol: symbol.to_string(),
            code: symbol[2..].to_string(),
            market: Some(Market::Stock),
            exchange: Some(Exchange::Sse),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn validate_symbol_uses_exact_match_instead_of_first_search_result() {
        let provider =
            MockProvider { search_results: vec![stock("SH600000"), stock("SH601318")], ..Default::default() };

        let investment = validate_symbol(&provider, "SH601318").await.unwrap();

        assert_eq!(investment.symbol, "SH601318");
    }

    #[tokio::test]
    async fn validate_symbol_rejects_fuzzy_only_results() {
        let provider = MockProvider { search_results: vec![stock("SH600000")], ..Default::default() };

        assert!(validate_symbol(&provider, "SH601318").await.is_err());
    }

    #[tokio::test]
    async fn validate_symbol_preserves_us_class_separator_in_search_query() {
        let expected = "BRK.A".parse::<Investment>().unwrap();
        let provider = MockProvider { search_results: vec![expected], ..Default::default() };

        let investment = validate_symbol(&provider, "BRK.A").await.unwrap();

        assert_eq!(investment.symbol, "$BRK.A");
        assert_eq!(provider.search_queries.lock().unwrap().as_slice(), ["BRK.A"]);
    }

    #[tokio::test]
    async fn search_command_rejects_zero_limit_before_remote_request() {
        let provider = MockProvider::default();

        let error = handle_search(&provider, "test", 0).await.unwrap_err();

        assert!(error.to_string().contains("大于 0"));
        assert!(provider.search_queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quote_command_rejects_empty_input_and_partial_snapshot() {
        let provider = MockProvider::default();
        let options = QuoteOptions { no_check: true, realtime: false, multiline: false };
        assert!(handle_quote(&provider, ",,", options).await.is_err());

        let options = QuoteOptions { no_check: true, realtime: false, multiline: false };
        assert!(handle_quote(&provider, "SH601318", options).await.is_err());
    }

    #[tokio::test]
    async fn quote_command_rejects_multiline_for_multiple_symbols() {
        let provider = MockProvider::default();
        let options = QuoteOptions { no_check: true, realtime: true, multiline: true };

        let error = handle_quote(&provider, "SH601318,SZ000001", options).await.unwrap_err();

        assert!(error.to_string().contains("仅支持单个证券"));
        assert!(provider.search_queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quote_command_rejects_multiline_without_realtime() {
        let provider = MockProvider::default();
        let options = QuoteOptions { no_check: true, realtime: false, multiline: true };

        let error = handle_quote(&provider, "SH601318", options).await.unwrap_err();

        assert!(error.to_string().contains("--realtime"));
        assert!(provider.quote_queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quote_command_is_all_or_nothing_and_deduplicates_before_search() {
        let provider = MockProvider { search_results: vec![stock("SH601318")], ..Default::default() };
        let options = QuoteOptions { no_check: false, realtime: false, multiline: false };

        let error = handle_quote(&provider, "SH601318,sh601318,SH999999", options).await.unwrap_err();

        assert!(error.to_string().contains("SH999999"));
        assert_eq!(provider.search_queries.lock().unwrap().as_slice(), ["SH601318", "SH999999"]);
        assert!(provider.quote_queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quote_command_limits_cardinality_before_remote_requests() {
        let provider = MockProvider::default();
        let symbols = (0..=64).map(|code| format!("SH{code:06}")).collect::<Vec<_>>().join(",");
        let options = QuoteOptions { no_check: false, realtime: false, multiline: false };

        let error = handle_quote(&provider, &symbols, options).await.unwrap_err();

        assert!(error.to_string().contains("最多查询 64"));
        assert!(provider.search_queries.lock().unwrap().is_empty());
        assert!(provider.quote_queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quote_command_deduplicates_no_check_snapshot_requests() {
        let provider = MockProvider::default();
        let options = QuoteOptions { no_check: true, realtime: false, multiline: false };

        assert!(handle_quote(&provider, "SH601318, sh601318", options).await.is_err());
        assert_eq!(provider.quote_queries.lock().unwrap().as_slice(), ["SH601318"]);
    }

    #[tokio::test]
    async fn realtime_provider_propagates_callback_errors() {
        let provider = MockProvider::default();

        let error = provider.quotes_realtime("SH601318", |_| bail!("callback failed")).await.unwrap_err();

        assert_eq!(error.to_string(), "callback failed");
    }
}
