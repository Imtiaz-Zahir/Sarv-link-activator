use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::windows::ffi::OsStrExt;
use std::process::{Command, Stdio};
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
    println!("⏳ Extracting embedded token...");

    let exe_path = std::env::current_exe()
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "Embedded token not found"))?;

    let mut exe_file = File::open(&exe_path)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "Embedded token not found"))?;

    let mut buffer = Vec::new();
    exe_file
        .read_to_end(&mut buffer)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Embedded token not found"))?;

    let magic = b"TOKEN_START::";
    let end_magic = b"::TOKEN_END";

    let token = buffer
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
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Embedded token not found"))?;

    println!("✅ Token extracted successfully");
    Ok(token)
}

fn os_str_to_wide(os_str: &OsStr) -> Vec<u16> {
    os_str.encode_wide().chain(Some(0)).collect()
}

fn execute_powershell_command(command: &str) -> io::Result<()> {
    println!("⚙️ Executing command: {}", command);

    let output = Command::new("powershell")
        .args(["-Command", command])
        .stdout(Stdio::inherit()) // Inherit stdout so output is printed as usual
        .stderr(Stdio::piped()) // Capture stderr separately
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("{}", stderr);

    if stderr.is_empty() || output.status.success() {
        println!("✅ Command executed successfully");
        Ok(())
    } else {
        println!("❌ Command execution failed");
        Err(io::Error::new(io::ErrorKind::Other, stderr))
    }
}

fn prompt_exit() {
    println!("\nOperation completed. Press any key to exit...");
    let _ = std::process::Command::new("cmd")
        .args(["/C", "pause"])
        .status();
}

fn is_winget_installed() -> io::Result<bool> {
    let output = std::process::Command::new("winget")
        .arg("--version")
        .output();

    match output {
        Ok(output) => Ok(output.status.success()),
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(e)
            }
        }
    }
}

fn uninstall_cloudflared_service() -> io::Result<()> {
    println!("⚙️ Uninstalling Cloudflared service...");

    // Command to uninstall the Cloudflared service
    let uninstall_command = "Start-Process powershell -ArgumentList '-Command', 'cloudflared.exe service uninstall' -NoNewWindow -Wait";

    // Spawn a new PowerShell instance to run the uninstall command
    let status = std::process::Command::new("powershell")
        .args(&["-Command", uninstall_command])
        .status()?;

    if status.success() {
        println!("✅ Cloudflared service uninstalled successfully.");
        Ok(())
    } else {
        println!("❌ Failed to uninstall Cloudflared service.");
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Failed to uninstall Cloudflared service",
        ))
    }
}

fn main() -> io::Result<()> {
    let is_admin = unsafe { IsUserAnAdmin() } != 0;

    if !is_admin {
        println!("⚠️  Administrator privileges required");
        println!("🔼 Requesting elevation...");

        let exe_path = std::env::current_exe()?;
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
            println!("❌ Failed to elevate privileges");
            prompt_exit();
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Failed to elevate privileges",
            ));
        }
        Ok(())
    } else {
        println!("🔒 Running with administrator privileges");

        let token = match extract_token() {
            Ok(t) => t,
            Err(e) => {
                println!("❌ Error: {}", e);
                prompt_exit();
                return Err(e);
            }
        };

        println!("🔍 Checking for winget installation...");
        if !is_winget_installed()? {
            println!("📦 Winget not found. Installing...");
            let install_cmd = r"
                $ProgressPreference = 'SilentlyContinue'
                $url = 'https://aka.ms/getwinget'
                $output = Join-Path $env:TEMP 'Microsoft.DesktopAppInstaller.msixbundle'
                try {
                    Invoke-WebRequest -Uri $url -OutFile $output -UseBasicParsing
                    Add-AppxPackage -Path $output -ErrorAction Stop
                    Write-Host '✅ Winget installed successfully'
                } catch {
                    Write-Error $_
                    exit 1
                }
            ";

            if let Err(e) = execute_powershell_command(install_cmd) {
                println!("❌ Failed to install winget: {}", e);
                prompt_exit();
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
        }

        println!("🚀 Installing Cloudflared...");
        let install_command = format!("winget install --id Cloudflare.cloudflared");
        if let Err(e) = execute_powershell_command(&install_command) {
            println!("❌ Failed to install Cloudflared: {}", e);
            prompt_exit();
            return Err(io::Error::new(io::ErrorKind::Other, e));
        }

        println!("⚙️ Configuring Cloudflared service...");
        let configure_command = format!("cloudflared.exe service install {}", token);
        match execute_powershell_command(&configure_command) {
            Err(e) => {
                if e.to_string().contains("service is already installed") {
                    println!("You already have a Cloudflared service installed. To install a new service you must uninstall the previous one first.");
                    println!("Do you want to uninstall the service? (y/n)");
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    if input.trim().eq_ignore_ascii_case("y") {
                        if let Err(e) = uninstall_cloudflared_service() {
                            println!("❌ Failed to uninstall service: {}", e);
                            prompt_exit();
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }

                        if let Err(e) = execute_powershell_command(&configure_command) {
                            println!("❌ Failed to configure service: {}", e);
                            prompt_exit();
                            return Err(io::Error::new(io::ErrorKind::Other, e));
                        }
                    } else {
                        println!("❌ Service uninstallation aborted by user.");
                        prompt_exit();
                        return Ok(());
                    }
                } else {
                    println!("❌ Failed to configure service: {}", e);
                    prompt_exit();
                    return Err(io::Error::new(io::ErrorKind::Other, e));
                }
            }
            Ok(_) => {
                println!("✅ Cloudflared service configured successfully.");
            }
        }

        println!("\n🎉 All operations completed successfully!");
        prompt_exit();
        Ok(())
    }
}
