use adw::prelude::*; // Use Adwaita prelude
use adw::{
    Application, ApplicationWindow, Clamp, EntryRow, HeaderBar, PreferencesGroup, WindowTitle,
};
use gtk::glib; // For channels and async

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use gtk::{
    gdk::{Key, ModifierType},
    Align,
    Box, // Use gtk::Box for the main container
    Button,
    EventControllerKey,
    Orientation,
    ScrolledWindow,
};

// Import necessary traits
use adw::prelude::WidgetExt;

const APP_ID: &str = "com.github.bjesus.putput";

// Enum for messages sent from background thread to main thread
enum CommandUpdate {
    Output(String, String), // Command Name, Output/Error String
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Config {
    run_commands_on_change: bool,
    commands: Vec<String>,
    title: String, // Added title field to Config
}

impl Default for Config {
    fn default() -> Self {
        Config {
            run_commands_on_change: false,
            commands: vec!["cat".to_string(), "wc".to_string()],
            title: "Putput".to_string(), // Default title
        }
    }
}

fn main() {
    // Initialize Libadwaita (and GTK implicitly)
    adw::init().expect("Failed to initialize Libadwaita");

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(build_ui);

    app.run();
}

fn build_ui(app: &Application) {
    // Load configuration
    let config = load_config();
    // Use Arc for sharing config data with closures/threads
    let config = Arc::new(config);

    // Use the title from the config
    let app_title = config.title.clone();

    // Create UI elements using Adwaita/GTK
    let window = ApplicationWindow::builder()
        .application(app)
        .title(&app_title) // Use the title from config
        .default_width(400)
        .default_height(500)
        .build();

    // Create the manual HeaderBar widget
    let header_bar = HeaderBar::builder()
        .title_widget(&WindowTitle::new(&app_title, "")) // Use the title from config
        .build();

    // --- Header Bar Buttons ---
    let clear_button = Button::from_icon_name("edit-clear-symbolic");
    clear_button.set_tooltip_text(Some("Clear Input"));
    // Add the clear button to the start of the manual HeaderBar
    header_bar.pack_start(&clear_button);

    // Create a vertical box to hold the header bar and the main content area
    let main_vbox = Box::new(Orientation::Vertical, 0); // 0 spacing between children

    // Add the manual header bar to the top of the main vertical box
    main_vbox.append(&header_bar);

    // Create main content box inside a Clamp for responsive width
    let content_box = Box::new(Orientation::Vertical, 10); // 10 spacing between content elements
    content_box.set_margin_start(10);
    content_box.set_margin_end(10);
    content_box.set_margin_top(10);
    content_box.set_margin_bottom(10);
    content_box.set_vexpand(true); // Allow content box to expand vertically

    let clamp = Clamp::builder().child(&content_box).build();
    clamp.set_vexpand(true); // Allow clamp to expand vertically

    // Add the clamp (containing the main input/output content) to the rest of the main vertical box
    main_vbox.append(&clamp);
    main_vbox.set_vexpand(true); // Allow the main vertical box to expand vertically

    // --- Input Area ---
    // Use AdwEntryRow for the input area
    let input_entry_row = EntryRow::builder()
        .title("Input") // Updated title text
        .build();

    input_entry_row.set_margin_top(5);
    input_entry_row.set_margin_bottom(5);
    input_entry_row.set_margin_start(5);
    input_entry_row.set_margin_end(5);

    content_box.append(&input_entry_row);

    // --- Output Area ---
    // Use PreferencesGroup for styled grouping of outputs
    let output_group = PreferencesGroup::new();
    output_group.set_margin_top(5);
    output_group.set_margin_bottom(5);
    output_group.set_margin_start(5);
    output_group.set_margin_end(5);

    let output_scroll = ScrolledWindow::new();
    output_scroll.set_vexpand(true); // Allow output area to expand vertically
    output_scroll.set_min_content_height(200); // Minimum height for the output area
    output_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic); // Only show vertical scrollbar when needed
    output_scroll.set_child(Some(&output_group)); // Set the output group as the child of the scrolled window
    content_box.append(&output_scroll);

    // Channel for async communication between command threads and UI thread
    let (sender, receiver) = async_channel::unbounded::<CommandUpdate>();

    // Configure command output sections using AdwEntryRow
    // Store AdwEntryRow widgets directly for easier updates from the receiver
    let command_output_rows: Arc<Vec<(String, EntryRow)>> = Arc::new(
        config
            .commands
            .iter()
            .map(|cmd| {
                // Use AdwEntryRow for each command's output
                let output_entry_row = EntryRow::builder()
                    .title(cmd) // Use command as the title of the EntryRow
                    .editable(false) // Output should not be editable
                    .build();

                // Create a Copy button for this command's output
                let copy_button = Button::from_icon_name("edit-copy-symbolic");
                copy_button.set_tooltip_text(Some("Copy Output"));
                copy_button.set_valign(Align::Center); // Vertically align the copy button

                // Clone the EntryRow for the copy button's click handler to get its text
                let output_entry_row_clone = output_entry_row.clone();

                // Connect the clicked signal of the copy button
                copy_button.connect_clicked(move |_| {
                    let text = output_entry_row_clone.text(); // Get text directly from EntryRow using EntryExt
                    if let Some(display) = gtk::gdk::Display::default() {
                        // Get the default GDK display and its clipboard
                        display.clipboard().set_text(&text); // Set the clipboard text
                    }
                });

                // Add the copy button as a suffix to the EntryRow
                output_entry_row.add_suffix(&copy_button);

                // Add the output EntryRow to the output group
                output_group.add(&output_entry_row);

                (cmd.clone(), output_entry_row) // Store command name and its EntryRow
            })
            .collect(),
    );

    // --- Connect Signals ---

    // Receiver for updates from background threads
    let command_output_rows_clone = Arc::clone(&command_output_rows);
    glib::spawn_future_local(async move {
        // Use glib::spawn_future_local for futures that interact with the GTK main loop
        while let Ok(update) = receiver.recv().await {
            match update {
                CommandUpdate::Output(cmd_name, output_text) => {
                    // Find the corresponding EntryRow and update its text on the main thread
                    if let Some((_, entry_row)) = command_output_rows_clone
                        .iter()
                        .find(|(name, _)| name == &cmd_name)
                    {
                        entry_row.set_text(&output_text); // Set the text of the EntryRow using EntryExt
                    }
                }
            }
        }
    });

    // Function to trigger commands (used by button and key press)
    let trigger_run_commands = {
        let input_entry_row_clone = input_entry_row.clone(); // Clone the EntryRow
        let config_clone = Arc::clone(&config);
        let sender_clone = sender.clone();
        let command_output_rows_clone = Arc::clone(&command_output_rows); // Clone for clearing outputs
        move || {
            // Clear previous outputs before running new commands for a clean view
            for (_, entry_row) in command_output_rows_clone.iter() {
                entry_row.set_text("");
            }

            // Get text directly from the input EntryRow using EntryExt
            let text = input_entry_row_clone.text();

            // Spawn the async command execution
            run_commands_async(
                text.to_string(), // Convert GString to String
                Arc::clone(&config_clone),
                sender_clone.clone(),
            );
        }
    };

    input_entry_row.connect_entry_activated(move |_| trigger_run_commands());

    let config_clone = Arc::clone(&config);
    let sender_clone = sender.clone();
    let command_output_rows_clone_for_change = Arc::clone(&command_output_rows); // Clone for clearing outputs on change

    // Connect to the 'changed' signal directly on the input EntryRow
    if config_clone.run_commands_on_change {
        input_entry_row.connect_changed(move |entry_row| {
            // Check if the config setting for running on change is enabled
            // Clear previous outputs before running on change for a clean view
            for (_, entry_row) in command_output_rows_clone_for_change.iter() {
                entry_row.set_text("");
            }

            // Get text directly from the EntryRow passed to the signal handler
            let text = entry_row.text();

            // Spawn the async command execution
            run_commands_async(
                text.to_string(), // Convert GString to String
                Arc::clone(&config_clone),
                sender_clone.clone(),
            );
        });
    }

    // Connect Clear Button signal
    let input_entry_row_clone_for_clear = input_entry_row.clone(); // Clone EntryRow for this closure
    let command_output_rows_clone_for_clear = Arc::clone(&command_output_rows); // Clone for clear button
    clear_button.connect_clicked(move |_| {
        input_entry_row_clone_for_clear.set_text(""); // Clear the input EntryRow using EntryExt
                                                      // Clear output fields as well for a clean state
        for (_, entry_row) in command_output_rows_clone_for_clear.iter() {
            entry_row.set_text("");
        }
    });

    // --- Ctrl+Number Copy Shortcuts ---
    // This controller remains on the window for global shortcuts
    let key_controller_copy = EventControllerKey::new(); // Controller for copy shortcuts
    let command_output_rows_clone_for_copy = Arc::clone(&command_output_rows); // Clone for copy handler

    key_controller_copy.connect_key_pressed(move |_, keyval, _, modifier| {
        // Check for Ctrl modifier
        if modifier.contains(ModifierType::CONTROL_MASK) {
            // Check if the key is a number key (1-9) using the correct Key variants
            let index = match keyval {
                Key::_1 => Some(0),
                Key::_2 => Some(1),
                Key::_3 => Some(2),
                Key::_4 => Some(3),
                Key::_5 => Some(4),
                Key::_6 => Some(5),
                Key::_7 => Some(6),
                Key::_8 => Some(7),
                Key::_9 => Some(8),
                // Also check Numpad keys
                Key::KP_1 => Some(0),
                Key::KP_2 => Some(1),
                Key::KP_3 => Some(2),
                Key::KP_4 => Some(3),
                Key::KP_5 => Some(4),
                Key::KP_6 => Some(5),
                Key::KP_7 => Some(6),
                Key::KP_8 => Some(7),
                Key::KP_9 => Some(8),
                _ => None, // Not a number key we care about
            };

            if let Some(index) = index {
                // Safely access the command_output_rows vector
                if let Some((_, entry_row)) = command_output_rows_clone_for_copy.get(index) {
                    let text = entry_row.text(); // Get text from the EntryRow
                    if let Some(display) = gtk::gdk::Display::default() {
                        display.clipboard().set_text(&text); // Set the clipboard text
                    }
                    glib::Propagation::Stop // Stop propagation as we handled the shortcut
                } else {
                    // Index is out of bounds (e.g., Ctrl+3 but only 2 commands defined)
                    println!("No command output available for index {}.", index); // Optional feedback
                    glib::Propagation::Proceed // Let other handlers potentially process
                }
            } else {
                glib::Propagation::Proceed // Not a Ctrl+Number shortcut
            }
        } else {
            glib::Propagation::Proceed // Ctrl key not pressed
        }
    });
    window.add_controller(key_controller_copy); // Add the copy key controller to the window

    // Set main content and show window
    window.set_content(Some(&main_vbox));

    // Set initial focus to the input EntryRow after the window is presented
    // Using grab_focus() requests focus. GTK will handle it when possible.
    window.present(); // Present the window first
    input_entry_row.grab_focus(); // Request focus for the input EntryRow
}

// Runs commands in separate threads and sends updates via channel
fn run_commands_async(
    input: String,
    config: Arc<Config>,
    sender: async_channel::Sender<CommandUpdate>,
) {
    // Iterate over each command defined in the configuration
    for cmd_str in config.commands.iter() {
        let command = cmd_str.clone(); // Clone the command string for the thread
        let input_clone = input.clone(); // Clone the input string for the thread
        let sender_clone = sender.clone(); // Clone the channel sender for the thread

        // Spawn a new OS thread to execute the command in the background
        thread::spawn(move || {
            // Execute the command and get the output
            let output = execute_command(&command, &input_clone);
            // Send the command name and its output back to the main thread via the channel
            // Use send_blocking because we are in a synchronous thread
            if let Err(e) = sender_clone.send_blocking(CommandUpdate::Output(command, output)) {
                eprintln!("Failed to send command output to main thread: {}", e);
            }
        });
    }
}

// Executes a single command, writes input to its stdin, and captures stdout/stderr
fn execute_command(cmd_str: &str, input: &str) -> String {
    // Split the command string into program name and arguments
    let cmd_parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if cmd_parts.is_empty() {
        return "Error: Empty command".to_string();
    }

    let program = cmd_parts[0]; // The first part is the program name
    let args = &cmd_parts[1..]; // The rest are arguments

    // Attempt to spawn the command
    match Command::new(program)
        .args(args) // Pass the arguments
        .stdin(Stdio::piped()) // Pipe stdin so we can write to it
        .stdout(Stdio::piped()) // Pipe stdout to capture output
        .stderr(Stdio::piped()) // Pipe stderr to capture errors
        .spawn() // Spawn the child process
    {
        Ok(mut child) => {
            // If the command spawned successfully, write input to its stdin
            if let Some(mut stdin) = child.stdin.take() {
                // Take ownership of stdin handle
                match stdin.write_all(input.as_bytes()) {
                    Ok(_) => {} // Writing successful
                    Err(e) => return format!("Error writing to stdin: {}", e), // Handle write error
                }
                drop(stdin); // Explicitly drop stdin to close the pipe, signaling end of input to the child
            }

            // Wait for the command to finish and collect its output
            match child.wait_with_output() {
                Ok(output) => {
                    // Check if the command exited successfully
                    if output.status.success() {
                        // If successful, return the standard output as a String
                        // Trim trailing whitespace (including newlines)
                        String::from_utf8_lossy(&output.stdout).trim_end().to_string()
                    } else {
                        // If failed, return the status code and standard error as a formatted String
                        // Trim trailing whitespace (including newlines) from stderr as well
                        format!(
                            "Failed ({}):\n{}",
                            output.status,
                            String::from_utf8_lossy(&output.stderr).trim_end()
                        )
                    }
                }
                Err(e) => format!("Failed to get command output: {}", e), // Handle error waiting for output
            }
        }
        Err(e) => format!("Failed to execute '{}': {}", cmd_str, e), // Handle error spawning command
    }
}

// --- Config Loading and Saving ---

// Gets the path to the configuration file following XDG Base Directory Specification
fn load_config() -> Config {
    let config_path = get_config_path();
    // Attempt to read the config file
    match fs::read_to_string(&config_path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(config) => {
                println!("Loaded config from {:?}", config_path);
                config // Return the parsed config
            }
            Err(e) => {
                // Handle TOML parsing errors
                eprintln!(
                    "Error parsing config file {:?}: {}. Using default.",
                    config_path, e
                );
                let default_config = Config::default();
                // Attempt to write a default config (might fix a corrupted file)
                write_default_config(&config_path, &default_config);
                default_config // Return the default config
            }
        },
        Err(_) => {
            // Handle file not found or read errors
            println!(
                "Config file not found at {:?}. Creating default.",
                config_path
            );
            let default_config = Config::default();
            // Write the default config
            write_default_config(&config_path, &default_config);
            default_config // Return the default config
        }
    }
}

// Determines the configuration file path
fn get_config_path() -> PathBuf {
    // Use the dirs crate to find the user's configuration directory
    let mut config_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".")) // Fallback to current directory if config dir not found
        .join("putput"); // Create an app-specific subdirectory

    // Create the config directory if it doesn't exist (ignore result of create_dir_all)
    let _ = fs::create_dir_all(&config_path);

    // Append the config file name
    config_path.push("config.toml");
    config_path
}

// Writes the default configuration to the specified path
fn write_default_config(path: &PathBuf, config: &Config) {
    // Serialize the config to a pretty TOML string
    match toml::to_string_pretty(config) {
        Ok(toml_str) => {
            // Ensure the parent directory exists before writing the file
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!("Error creating config directory {:?}: {}", parent, e);
                        return; // Stop if directory creation fails
                    }
                }
            }
            // Write the TOML string to the file
            if let Err(e) = fs::write(path, toml_str) {
                eprintln!("Error writing default config file {:?}: {}", path, e);
            } else {
                println!("Created default config at {:?}", path);
            }
        }
        Err(e) => {
            eprintln!("Error serializing default config: {}", e); // Handle serialization errors
        }
    }
}
