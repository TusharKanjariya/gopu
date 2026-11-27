//! Prints the focused field and its on-screen context: `cargo run -p gopu-core --example focused_field`
fn main() {
    let Some(field) = gopu_core::platform::focused_field() else { return println!("no field focused") };
    println!("rect {:?} caret {:?}\ntext {:?}\nselection {:?}\n", field.rect, field.caret, field.text, field.selection);
    let ctx = gopu_core::platform::on_screen_context(&field);
    println!("app: {} | {}\nwalk {} ms, field found: {}", ctx.app, ctx.window_title, ctx.took_ms, ctx.anchored);
    println!("--- last 300 chars before ---\n{}", ctx.before.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect::<String>());
}
