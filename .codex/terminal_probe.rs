use std::ffi::OsString;
use shadow_terminal::{shadow_terminal::Config, steppable_terminal::SteppableTerminal};

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    runtime.block_on(async move {
        let config = Config {
            width: 120,
            height: 32,
            command: vec![OsString::from("powershell.exe"), OsString::from("-NoLogo")],
            scrollback_size: 3000,
            scrollback_step: 5,
        };
        let mut terminal = SteppableTerminal::start(config).await.unwrap();
        terminal.render_all_output().await.unwrap();
        println!("INITIAL:\n{}", terminal.screen_as_string().unwrap());
        terminal.send_command("echo hello").unwrap();
        for _ in 0..100 {
            terminal.render_all_output().await.unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!("AFTER_SEND_COMMAND:\n{}", terminal.screen_as_string().unwrap());
        terminal.send_command_with_osc_paste("echo osc").unwrap();
        for _ in 0..100 {
            terminal.render_all_output().await.unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!("AFTER_OSC:\n{}", terminal.screen_as_string().unwrap());
        terminal.kill().unwrap();
    });
}
