# Rains

[![CI](https://github.com/rookie0/rains/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/rookie0/rains/actions)
[![Crates.io](https://img.shields.io/crates/v/rains.svg)](https://crates.io/crates/rains)

💹 命令行股票信息行情数据查询工具

支持 A 股（沪深北）、港股、美股的实时行情查询，以及 A 股公司财务信息查询。数据来源：新浪财经

## 功能特性

- 🔍 **搜索** - 支持拼音、代码、名称模糊搜索
- 📊 **行情** - 实时/快照行情，支持多股票同时查询
- 📈 **信息** - 公司基本信息、财务指标、股东结构、分红送配、公告

## 安装

### 下载二进制

到 [Releases](https://github.com/rookie0/rains/releases) 下载对应平台的包

### Cargo 安装

```bash
cargo install rains
```

### 从源码编译

需要 Rust 1.86 或更高版本。

```bash
git clone https://github.com/rookie0/rains.git
cd rains
cargo build --release --locked
```

## 使用

```
Usage: rains [OPTIONS] <COMMAND>

Commands:
  search  搜索股票 [别名: s]
  info    股票信息 [别名: i]
  quote   行情报价 [别名: q]
  help    帮助信息

Options:
  -d, --debug    开启调试日志
  -h, --help     帮助信息
  -V, --version  版本信息
```

### 搜索股票

```bash
rains search zgpa          # 拼音搜索
rains s 平安               # 名称搜索
rains s 601318             # 代码搜索
rains s baba -l 5          # 限制返回条数
```

### 行情查询

```bash
# A 股
rains quote SH601318              # 上证
rains quote SZ000001              # 深证
rains quote BJ430047              # 北证

# 港股
rains q HK00700                   # 腾讯
rains q HKHSI                     # 恒生指数

# 美股（$ 前缀需转义或用引号，也可不加）
rains q '$BABA'                   # 阿里巴巴
rains q AAPL                      # 苹果

# 多股票查询
rains q SH601318,SZ000001,HK00700

# 实时行情
rains q SH601318 -r               # 持续刷新
rains q SH601318 -r -m            # 多行展示（仅单股票）
```

### 信息查询

```bash
rains info SH601318        # 基本信息
rains i SH601318 -a        # 全部信息
rains i SH601318 -f        # 财务指标
rains i SH601318 -s        # 股东结构
rains i SH601318 -d        # 分红送配
rains i SH601318 -p        # 最新公告
```

> ⚠️ 信息查询目前仅支持 A 股（沪深北）

## 项目结构

```
src/
├── main.rs           # 入口
├── cli.rs            # 命令行参数
├── format.rs         # 输出格式化
├── commands/         # 命令处理
│   ├── search.rs
│   ├── info.rs
│   └── quote.rs
├── invest/           # 投资品类型
│   ├── mod.rs
│   ├── quote.rs
│   └── stock.rs
└── provider/         # 数据源
    ├── mod.rs        # Provider trait
    └── sina.rs       # 新浪财经实现
```

## 开发

```bash
# 运行测试
cargo test --locked --all-targets

# 代码检查
cargo clippy --locked --all-targets --all-features -- -D warnings

# 格式化
cargo fmt

# 调试运行
cargo run -- -d search zgpa
```

## License

[MIT](LICENSE)
