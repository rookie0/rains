
# Rains

[![CI](https://github.com/rookie0/rains/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/rookie0/rains/actions) [![Crates.io](https://img.shields.io/crates/v/rains.svg)](https://crates.io/crates/rains)

💹 命令行 A 股沪深北证股票信息行情数据查询工具，提供股票实时行情及相关公司财务信息，数据来源新浪财经


## 安装

- 直接下载，到 [Releases](https://github.com/rookie0/rains/releases) 下载对应平台的包

- Cargo 安装，`cargo install rains`


## 使用

```
USAGE:
    rains [OPTIONS] <SUBCOMMAND>

OPTIONS:
    -d, --debug
    -h, --help       Print help information
    -V, --version    Print version information

SUBCOMMANDS:
    help      Print this message or the help of the given subcommand(s)
    info      股票信息
    quote     行情报价
    search    搜索股票
```

示例
```
rains search zgpa        搜索中国平安代码
rains info SH601318 -a   中国平安全部信息
rains quote SH601318 -r  中国平安实时行情

rains help info|quote|search      查看命令用法，子命令支持简写 i|q|s
rains q HK00700,HK09626,SH600519  支持港股行情（暂不支持港股信息查询）
rains q \$BILI,BABA,JD            支持美股行情（默认加 $ 前缀区分，命令行需转义，也可不加；暂不支持美股信息查询）
rains quote SH601318,SZ000001 -r  支持多只股票实时行情
```


## License

[MIT](LICENSE)
