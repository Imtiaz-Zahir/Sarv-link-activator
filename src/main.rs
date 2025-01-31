use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;

type HINSTANCE = *mut std::ffi::c_void;
const SW_SHOW: i32 = 5;

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: *mut std::ffi::c_void,
        lp_operation: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show_cmd: i32,
    ) -> HINSTANCE;
}

extern "system" {
    fn IsUserAnAdmin() -> i32;
}

fn extract_token() -> io::Result<String> {
    let exe_path = std::env::current_exe()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "Embedded token not found"))?;
    
    let mut exe_file = File::open(&exe_path)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "Embedded token not found"))?;
    
    let mut buffer = Vec::new();
    exe_file.read_to_end(&mut buffer)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Embedded token not found"))?;

    let magic = b"TOKEN_START::";
    let end_magic = b"::TOKEN_END";

    buffer
        .windows(magic.len())
        .position(|w| w == magic)
        .and_then(|start| {
            let token_start = start + magic.len();
            buffer[token_start..]
                .windows(end_magic.len())
                .position(|w| w == end_magic)
                .map(|end| (token_start, end))
        })
        .map(|(start, end)| String::from_utf8_lossy(&buffer[start..start + end]).into_owned())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Embedded token not found"))
}

fn os_str_to_wide(os_str: &OsStr) -> Vec<u16> {
    os_str.encode_wide().chain(Some(0)).collect()
}

fn execute_powershell_command(command: &str) -> io::Result<()> {
    let status = std::process::Command::new("powershell")
        .args(["-Command", command])
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Command execution failed",
        ))
    }
}

fn prompt_exit() {
    println!("\nExiting...");
    let _ = std::process::Command::new("cmd")
        .args(["/C", "pause"])
        .status();
}

fn main() {
    let is_admin = unsafe { IsUserAnAdmin() } != 0;

    if !is_admin {
        let exe_path = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => {
                eprintln!("Error extracting token: Embedded token not found");
                prompt_exit();
                std::process::exit(1);
            }
        };

        let exe_path_wide = os_str_to_wide(exe_path.as_os_str());
        let verb_wide = os_str_to_wide(OsStr::new("runas"));

        let result = unsafe {
            ShellExecuteW(
                null_mut(),
                verb_wide.as_ptr(),
                exe_path_wide.as_ptr(),
                null_mut(),
                null_mut(),
                SW_SHOW,
            )
        };

        if (result as usize) <= 32 {
            eprintln!("Error extracting token: Embedded token not found");
            prompt_exit();
            std::process::exit(1);
        }
        return;
    }

    let token = match extract_token() {
        Ok(t) => t,
        Err(_) => {
            eprintln!("Error extracting token: Embedded token not found");
            prompt_exit();
            std::process::exit(1);
        }
    };

    let command = format!(
        "winget install --id Cloudflare.cloudflared; \
        cloudflared.exe service uninstall; \
        cloudflared.exe service install {}",
        token
    );

    if let Err(_) = execute_powershell_command(&command) {
        eprintln!("Command execution failed");
        prompt_exit();
        std::process::exit(1);
    }

    prompt_exit();
}