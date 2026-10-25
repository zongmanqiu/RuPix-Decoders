//! 打印 rawler 实际支持的扩展名（大写），用于校准插件 `extensions` 清单。
//!
//! 运行：`cargo run -p rupix-raw --example dump_exts`

fn main() {
    let exts = rawler::decoders::supported_extensions();
    for e in exts {
        println!("{}", e);
    }
}
