use std::process::Command;
use serde::Deserialize;
use inquire::Select;

// GitHub raw URL — kendi repo adresinle değiştir
const PACKAGES_URL: &str =
    "https://raw.githubusercontent.com/PisoyaruFan-code/arch-install-v2/main/files/packages.json";

#[derive(Deserialize, Debug)]
struct PackageList {
    core: Vec<String>,
    minimal: Vec<String>,
    desktop: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstallLevel {
    Core,
    Minimal,
    Desktop,
}

impl std::fmt::Display for InstallLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallLevel::Core    => write!(f, "Core    — Yalnızca temel sistem (base, kernel)"),
            InstallLevel::Minimal => write!(f, "Minimal — Core + geliştirici araçları, ağ, git"),
            InstallLevel::Desktop => write!(f, "Desktop — Minimal + masaüstü ortamı (Xorg, GNOME)"),
        }
    }
}

/// GitHub raw URL'den packages.json'u indirir.
/// Önce curl, yoksa wget ile dener.
fn fetch_package_list() -> Result<PackageList, String> {
    println!("📦 Paket listesi indiriliyor: {}", PACKAGES_URL);

    let json = try_curl(PACKAGES_URL)
        .or_else(|_| try_wget(PACKAGES_URL))
        .map_err(|e| format!("Paket listesi indirilemedi (curl ve wget başarısız): {}", e))?;

    serde_json::from_str(&json)
        .map_err(|e| format!("packages.json ayrıştırılamadı: {}", e))
}

fn try_curl(url: &str) -> Result<String, String> {
    let out = Command::new("curl")
        .args([
            "--silent",       // ilerleme çubuğu gösterme
            "--show-error",   // hata mesajlarını göster
            "--fail",         // HTTP hata kodlarında başarısız say (4xx, 5xx)
            "--location",     // redirect'leri takip et
            "--max-time", "15",
            url,
        ])
        .output()
        .map_err(|e| format!("curl çalıştırılamadı: {}", e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("curl hata döndürdü: {}", stderr));
    }

    String::from_utf8(out.stdout)
        .map_err(|e| format!("curl çıktısı UTF-8 değil: {}", e))
}

fn try_wget(url: &str) -> Result<String, String> {
    let out = Command::new("wget")
        .args([
            "--quiet",         // sessiz mod
            "--timeout=15",
            "--output-document=-", // stdout'a yaz
            url,
        ])
        .output()
        .map_err(|e| format!("wget çalıştırılamadı: {}", e))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("wget hata döndürdü: {}", stderr));
    }

    String::from_utf8(out.stdout)
        .map_err(|e| format!("wget çıktısı UTF-8 değil: {}", e))
}

/// Seçilen seviyeye göre kurulacak paket listesini oluşturur.
/// Kümülatif: Desktop → core + minimal + desktop hepsini içerir.
fn resolve_packages(list: &PackageList, level: &InstallLevel) -> Vec<String> {
    let mut packages = list.core.clone();

    if *level == InstallLevel::Minimal || *level == InstallLevel::Desktop {
        packages.extend(list.minimal.iter().cloned());
    }

    if *level == InstallLevel::Desktop {
        packages.extend(list.desktop.iter().cloned());
    }

    // Olası tekrarları temizle (JSON'da elle eklenmiş duplikasyonlara karşı)
    packages.sort();
    packages.dedup();

    packages
}

/// Kullanıcıya kurulum seviyesini seçtirir,
/// paketi indirir ve `pacstrap` ile kurar.
pub fn select_and_install_packages() -> Result<(), String> {
    // 1. Seviye seçimi
    let level = Select::new(
        "Kurulum seviyesini seçin:",
        vec![InstallLevel::Core, InstallLevel::Minimal, InstallLevel::Desktop],
    )
    .with_help_message("↑↓ hareket, Enter seç")
    .prompt()
    .map_err(|e| format!("Seçim iptal edildi: {}", e))?;

    // 2. Paket listesini indir
    let package_list = fetch_package_list()?;

    // 3. Kurulacak paketleri belirle
    let packages = resolve_packages(&package_list, &level);

    println!("\n📋 Kurulacak {} paket:", packages.len());
    for (i, pkg) in packages.iter().enumerate() {
        print!("  {}", pkg);
        if (i + 1) % 6 == 0 { println!(); }   // 6'da bir satır kır, okunabilirlik
    }
    println!("\n");

    // 4. pacstrap ile kur (/mnt önceden mount edilmiş olmalı)
    install_packages(&packages)
}

fn install_packages(packages: &[String]) -> Result<(), String> {
    println!("🚀 pacstrap başlatılıyor...");

    let status = Command::new("pacstrap")
        .arg("-K")      // yeni keyring başlat
        .arg("/mnt")
        .args(packages)
        .status()
        .map_err(|e| format!("pacstrap çalıştırılamadı: {}", e))?;

    if status.success() {
        println!("✅ Paketler başarıyla kuruldu.");
        Ok(())
    } else {
        Err(format!(
            "pacstrap başarısız oldu (Çıkış kodu: {:?})",
            status.code()
        ))
    }
}