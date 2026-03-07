// Quick test: does autopilot key simulation work on this machine?
// Run: cd src-tauri && cargo run --example test_keypress

fn main() {
    println!("Testing autopilot keyboard simulation...");
    println!("You have 3 seconds — click on a text field (e.g. TextEdit, Notes, browser URL bar)!");

    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("Typing 'hello from zeptobot'...");
    autopilot::key::type_string("hello from zeptobot", &[], 50.0, 0.0);

    println!("Done! Did text appear?");

    std::thread::sleep(std::time::Duration::from_secs(1));

    println!("\nNow testing Cmd+Space (Raycast)...");
    std::thread::sleep(std::time::Duration::from_secs(2));

    autopilot::key::tap(
        &autopilot::key::Code(autopilot::key::KeyCode::Space),
        &[autopilot::key::Flag::Meta],
        0,
        0,
    );

    println!("Pressed Cmd+Space. Did Raycast open?");
}
