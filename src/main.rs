mod draft;
mod bot;
mod profile;

fn main() -> cambc::Result<()> {
    let mut bot = bot::Bot::default();
    loop {
        if cambc::is_draft() {
            let _ = draft::play();
        } else {
            let _ = bot.play_turn();
        }
        cambc::end_turn();
    }
}