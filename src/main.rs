// GC-8：不加这行，GUI 背后会挂一个黑窗口 —— 正是本项目要消灭的东西。
// 用 cfg_attr(not(test)) 而非裸属性：避免测试构建也被标为 GUI 子系统而吞掉
// 测试输出。Task 2 的 Step 2 会验证这个写法确实让 `cargo test` 有输出。
#![cfg_attr(not(test), windows_subsystem = "windows")]

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    ui.run()
}
