// GC-8：不加这行，GUI 背后会挂一个黑窗口 —— 正是本项目要消灭的东西。
// 用 cfg_attr(not(test)) 而非裸属性：避免测试构建也被标为 GUI 子系统而吞掉
// 测试输出。Task 2 的 Step 2 会验证这个写法确实让 `cargo test` 有输出。
#![cfg_attr(not(test), windows_subsystem = "windows")]

// ⚠ 临时（Task 2 ~ Task 16 期间存在）
//
// 各模块按依赖顺序逐步落地，先定义的类型/函数要到很晚才被消费。
// 在【二进制 crate】中未使用的 pub 项会触发 dead_code 警告 ——
// 实测确认：bin crate 不会因为是 pub 就豁免这个 lint。
//
// 受影响的不止 model.rs：pm.rs 的 sorted_desc 要到 Task 11 才被消费、
// probe_env 要到 Task 18；dsh.rs 的 fetch_catalog 要到 Task 18；
// txn.rs 的 run 要到 Task 18。因此抑制必须是 crate 级的 ——
// 单独给 `mod model;` 加属性解决不了其余模块。
//
// 若不抑制，Task 2~16 的构建输出会持续带着十几个无关警告 ——
// 既让"输出必须干净"的检查失效，也会掩盖真实警告。
//
// 【Task 20 必须删除本行，并确认 cargo build 与 cargo test 都零警告】
// 本行会掩盖真实死代码（它覆盖面很宽），只应短期存在。
#![allow(dead_code)]

mod config;
mod model;
mod pm;
mod txn;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    ui.run()
}
