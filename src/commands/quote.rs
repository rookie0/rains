//! 行情报价命令

use std::{collections::HashSet, time::Duration};

use anyhow::{Result, bail};
use futures_util::{StreamExt, stream};
use tokio::time::timeout;

use crate::{
    commands::validate_symbol,
    format::{RealtimeQuotePrinter, print_quotes, sanitize_text},
    invest::Investment,
    provider::{MAX_QUOTE_SYMBOLS, Provider, RealtimeProvider},
};

const VALIDATION_CONCURRENCY: usize = 8;
const VALIDATION_TIMEOUT_SECS: u64 = 15;

/// 行情查询选项
pub struct QuoteOptions {
    pub no_check: bool,
    pub realtime: bool,
    pub multiline: bool,
}

/// 处理行情查询命令
pub async fn handle_quote<P: Provider + RealtimeProvider>(
    provider: &P,
    symbol: &str,
    options: QuoteOptions,
) -> Result<()> {
    if options.multiline && !options.realtime {
        bail!("--multiline 仅可与 --realtime 一起使用");
    }

    // 解析并验证代码列表
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut invalid = Vec::new();
    for s in symbol.split(',') {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }

        let parsed = match s.parse::<Investment>() {
            Ok(invest) => invest,
            Err(err) => {
                invalid.push(format!("{}: {}", sanitize_text(s), sanitize_text(&err.to_string())));
                continue;
            }
        };

        // 在远程校验前按规范化代码去重，避免重复 HTTP 查询和重复输出。
        if !seen.insert(parsed.symbol.clone()) {
            continue;
        }

        candidates.push((s.to_string(), parsed));
    }

    if !invalid.is_empty() {
        bail!("证券代码校验失败：{}", invalid.join("；"));
    }

    if candidates.is_empty() {
        bail!("没有可查询的证券代码");
    }
    if candidates.len() > MAX_QUOTE_SYMBOLS {
        bail!("单次最多查询 {MAX_QUOTE_SYMBOLS} 个证券代码");
    }
    if options.multiline && candidates.len() != 1 {
        bail!("实时行情多行展示仅支持单个证券");
    }

    let symbols = if options.no_check {
        candidates.into_iter().map(|(_, invest)| invest.symbol).collect::<Vec<_>>()
    } else {
        // 有界并发并保留输入顺序；总 deadline 防止多标的逐个超时导致分钟级挂起。
        let validations = stream::iter(candidates.into_iter().map(|(input, invest)| async move {
            (input, validate_symbol(provider, &invest.symbol).await.map(|validated| validated.symbol))
        }))
        .buffered(VALIDATION_CONCURRENCY)
        .collect::<Vec<_>>();
        let results = timeout(Duration::from_secs(VALIDATION_TIMEOUT_SECS), validations)
            .await
            .map_err(|_| anyhow::anyhow!("证券代码校验超时"))?;

        let mut symbols = Vec::with_capacity(results.len());
        for (input, result) in results {
            match result {
                Ok(symbol) => symbols.push(symbol),
                Err(err) => invalid.push(format!("{}: {}", sanitize_text(&input), sanitize_text(&err.to_string()))),
            }
        }
        if !invalid.is_empty() {
            bail!("证券代码校验失败：{}", invalid.join("；"));
        }
        symbols
    };

    let symbols_str = symbols.join(",");

    if options.realtime {
        handle_realtime_quotes(provider, &symbols_str, options.multiline).await?;
    } else {
        handle_snapshot_quotes(provider, &symbols_str).await?;
    }

    Ok(())
}

/// 处理实时行情
async fn handle_realtime_quotes<P: RealtimeProvider>(provider: &P, symbols: &str, multiline: bool) -> Result<()> {
    let mut printer = RealtimeQuotePrinter::new();
    // debug 日志与 ANSI 原地刷新共享终端时会互相覆盖，因此调试模式固定追加。
    let append_only = multiline || tracing::enabled!(tracing::Level::DEBUG);

    provider
        .quotes_realtime(symbols, move |quotes| {
            printer.print(&quotes, append_only)?;
            Ok(())
        })
        .await
}

/// 处理快照行情
async fn handle_snapshot_quotes<P: Provider>(provider: &P, symbols: &str) -> Result<()> {
    let quotes = provider.quotes(symbols).await?;
    if quotes.is_empty() {
        bail!("行情服务未返回有效数据");
    }
    print_quotes(&quotes)?;
    Ok(())
}
