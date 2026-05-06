use std::process::{Command, Stdio};
use std::io::Write;
use inquire::validator::Validation;
use inquire::{Confirm, Text};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct LsblkOutput {
    blockdevices: Vec<DiskInfo>,
}

#[derive(Deserialize, Debug)]
struct DiskInfo {
    name: String,
    size: String,
    model: Option<String>,
    #[serde(rename = "type")]
    device_type: String,
}

/// Boyut string'inin geçerli bir format içerip içermediğini kontrol eder.
/// Geçerli örnekler: "512MiB", "8GiB", "1TiB", "2048", "0"
fn is_valid_size(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Sadece rakamlardan oluşuyorsa geçerli (MiB olarak yorumlanacak)
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }

    // Rakam + geçerli birim kombinasyonu
    let valid_units = ["MiB", "GiB", "TiB", "KiB", "MB", "GB", "TB"];
    for unit in &valid_units {
        if let Some(num_part) = trimmed.strip_suffix(unit) {
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }

    false
}

/// Eğer kullanıcı sadece sayı girdiyse sonuna 'MiB' ekler,
/// yoksa olduğu gibi bırakır (sfdisk için hazırlık).
fn format_size(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        format!("{}MiB", trimmed)
    } else {
        trimmed.to_string()
    }
}

/// Swap boyutunun "sıfır" anlamına gelip gelmediğini kontrol eder.
fn is_zero_size(size: &str) -> bool {
    let lower = size.to_lowercase();
    lower == "0"
        || lower == "0mib"
        || lower == "0gib"
        || lower == "0kib"
        || lower == "0tib"
        || lower == "0mb"
        || lower == "0gb"
}

pub fn get_system_disks_sizes() -> (String, String) {
    // 1. EFI Boyutunu Al — geçersiz girişte tekrar sor
    let efi_size = loop {
        let input = Text::new("EFI (Boot) bölümü boyutu ne olsun?")
            .with_default("512MiB")
            .with_help_message("Örn: 1GiB, 512MiB. Sadece sayı girerseniz 'MiB' kabul edilir.")
            .with_validator(|input: &str| {
                if input.trim().is_empty() {
                    Ok(Validation::Invalid("Boyut boş bırakılamaz!".into()))
                } else if !is_valid_size(input) {
                    Ok(Validation::Invalid(
                        "Geçersiz format! Örn: 512MiB, 1GiB, 2048".into(),
                    ))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()
            .unwrap();

        let formatted = format_size(&input);
        if is_zero_size(&formatted) {
            println!("⚠️  EFI bölümü sıfır olamaz! Lütfen geçerli bir boyut girin.");
            continue;
        }
        break formatted;
    };

    // 2. Swap Boyutunu Al — geçersiz girişte tekrar sor
    let swap_size = loop {
        let input = Text::new("Swap (Takas alanı) boyutu ne olsun?")
            .with_default("8GiB")
            .with_help_message("Örn: 4GiB, 16GiB. Devre dışı bırakmak için 0 yazın.")
            .with_validator(|input: &str| {
                if input.trim().is_empty() {
                    Ok(Validation::Invalid("Boyut boş bırakılamaz!".into()))
                } else if !is_valid_size(input) {
                    Ok(Validation::Invalid(
                        "Geçersiz format! Örn: 8GiB, 4096MiB veya devre dışı için 0".into(),
                    ))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()
            .unwrap();

        break format_size(&input);
    };

    (efi_size, swap_size)
}

pub fn get_system_disks() -> Vec<String> {
    let output = Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,SIZE,MODEL,TYPE", "-J"])
        .output()
        .expect("lsblk komutu çalıştırılamadı");

    let decoded: LsblkOutput = serde_json::from_slice(&output.stdout)
        .expect("JSON ayrıştırılamadı");

    decoded
        .blockdevices
        .into_iter()
        .filter(|d| d.device_type == "disk")
        .map(|d| {
            format!(
                "/dev/{} - {} ({})",
                d.name,
                d.size,
                d.model.unwrap_or_else(|| "Bilinmeyen Model".to_string())
            )
        })
        .collect()
}

pub fn format_the_disk(device: &str) {
    let ans = Confirm::new(&format!(
        "DİKKAT: {} içindeki TÜM VERİLER silinecek. Emin misiniz?",
        device
    ))
    .with_default(false)
    .with_help_message("Bu işlem geri alınamaz!")
    .prompt();

    match ans {
        Ok(true) => {
            // Çift onay — kritik işlem için ek güvence
            let second = Confirm::new("Son kez soruyorum: Bu diski tamamen silmek istediğinizden emin misiniz?")
                .with_default(false)
                .prompt()
                .unwrap_or(false);

            if !second {
                println!("İşlem iptal edildi. Verileriniz güvende.");
                return;
            }

            println!("İmha süreci başlıyor...");

            println!("Mevcut bağlantılar kontrol ediliyor...");
            if let Err(e) = unmount_disk(device) {
                eprintln!("❌ Bağlantılar kaldırılamadı: {}", e);
                return;
            }

            if let Err(e) = wipe_disk(device) {
                eprintln!("❌ Disk temizlenemedi: {}", e);
                return;
            }

            let (efi_size, swap_size) = get_system_disks_sizes();
            let swap_enabled = !is_zero_size(&swap_size);

            if let Err(e) = partition_disk(device, &efi_size, &swap_size) {
                eprintln!("❌ Disk bölünemedi: {}", e);
                return;
            }

            if let Err(e) = format_partitions(device, swap_enabled) {
                eprintln!("❌ Bölümler formatlanamadı: {}", e);
                return;
            }

            if let Err(e) = mount_system(device, swap_enabled) {
                eprintln!("❌ Bölümler bağlanamadı: {}", e);
                return;
            }

            println!("✅ Tüm işlemler başarıyla tamamlandı.");
        }
        _ => {
            println!("İşlem iptal edildi. Verileriniz güvende.");
        }
    }
}

/// `/proc/mounts` dosyasını okuyarak verilen diske ait tüm mount noktalarını döndürür.
/// Örn: device="/dev/sda" → ["/dev/sda1 /mnt", "/dev/sda2 /mnt/boot"]
fn get_mounted_partitions(device: &str) -> Vec<(String, String)> {
    let content = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    content
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let part = parts.next()?;
            let mount = parts.next()?;
            // Sadece bu diske ait bölümler (örn: /dev/sda1, /dev/nvme0n1p2)
            if part.starts_with(device) {
                Some((part.to_string(), mount.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// `/proc/swaps` dosyasını okuyarak verilen diske ait aktif swap bölümlerini döndürür.
fn get_active_swaps(device: &str) -> Vec<String> {
    let content = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    content
        .lines()
        .skip(1) // başlık satırını atla
        .filter_map(|line| {
            let part = line.split_whitespace().next()?;
            if part.starts_with(device) {
                Some(part.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Diske ait tüm swap ve mount'ları temiz bir şekilde kaldırır.
/// Bağlı bölüm yoksa sessizce devam eder.
fn unmount_disk(device: &str) -> std::io::Result<()> {
    // 1. Aktif swap'ları kapat
    let swaps = get_active_swaps(device);
    if swaps.is_empty() {
        println!("  Aktif swap bulunamadı, atlanıyor.");
    } else {
        for swap in &swaps {
            println!("  Swap kapatılıyor: {}", swap);
            let s = Command::new("swapoff").arg(swap).status()?;
            if !s.success() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("swapoff başarısız: {}", swap),
                ));
            }
        }
    }

    // 2. Mount noktalarını bul — önce en derin yolu kapat (ters sıra)
    let mut mounts = get_mounted_partitions(device);
    if mounts.is_empty() {
        println!("  Bağlı bölüm bulunamadı, atlanıyor.");
    } else {
        // /mnt/boot → /mnt sırasıyla kapatmak için mount noktasına göre ters sırala
        mounts.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (part, mount) in &mounts {
            println!("  Bağlantı kesiliyor: {} ({})", part, mount);
            let s = Command::new("umount").arg(part).status()?;
            if !s.success() {
                // lazy unmount ile tekrar dene
                println!("  ⚠️  Normal umount başarısız, lazy unmount deneniyor: {}", part);
                let s2 = Command::new("umount").args(["-l", part]).status()?;
                if !s2.success() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("umount başarısız: {} → {}", part, mount),
                    ));
                }
            }
        }
    }

    Ok(())
}

fn wipe_disk(device: &str) -> std::io::Result<()> {
    println!("{} üzerindeki eski imzalar temizleniyor...", device);

    let status = Command::new("wipefs")
        .args(["--all", "--force", device])
        .status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "wipefs başarısız oldu!",
        ));
    }

    // Kernel'e bölüm tablosunun değiştiğini haber ver
    let zap = Command::new("sgdisk")
        .args(["--zap-all", device])
        .status()?;

    if !zap.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "sgdisk --zap-all başarısız oldu!",
        ));
    }

    Ok(())
}

pub fn partition_disk(device: &str, efi_size: &str, swap_size: &str) -> std::io::Result<()> {
    let swap_enabled = !is_zero_size(swap_size);
    let mut input = String::from("label: gpt\n");

    // 1. EFI Bölümü (ESP tipi: C12A7328-F81F-11D2-BA4B-00A0C93EC93B)
    input.push_str(&format!(
        "size={}, type=U, name=EFI\n",
        efi_size
    ));

    // 2. Swap Bölümü (opsiyonel)
    if swap_enabled {
        input.push_str(&format!("size={}, type=S, name=SWAP\n", swap_size));
    }

    // 3. Root Bölümü — geri kalan tüm alan
    input.push_str("type=L, name=ROOT\n");

    let mut child = Command::new("sfdisk")
        .arg(device)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }

    let status = child.wait()?;

    if status.success() {
        println!("✅ Disk başarıyla bölümlendi: {}", device);
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "sfdisk başarısız oldu (Çıkış kodu: {:?})",
                status.code()
            ),
        ))
    }
}

/// Bölüm numarasını döndürür.
/// swap_enabled=false ise ROOT, p2 olur (p3 değil).
fn part_num(index: u8, swap_enabled: bool) -> u8 {
    // index: 1=EFI, 2=SWAP(opsiyonel), 3=ROOT
    // swap kapalıysa ROOT için index 3 yerine 2 kullan
    match index {
        1 => 1,                                     // EFI her zaman 1
        2 if swap_enabled => 2,                     // SWAP
        2 => panic!("SWAP kapalı, bu çağrı geçersiz"),
        3 => if swap_enabled { 3 } else { 2 },      // ROOT
        _ => panic!("Geçersiz bölüm indexi"),
    }
}

fn part_path(device: &str, num: u8) -> String {
    let sep = if device.contains("nvme") || device.contains("mmcblk") {
        "p"
    } else {
        ""
    };
    format!("{}{}{}", device, sep, num)
}

fn format_partitions(device: &str, swap_enabled: bool) -> std::io::Result<()> {
    // 1. EFI (FAT32)
    let efi = part_path(device, part_num(1, swap_enabled));
    println!("EFI bölümü formatlanıyor: {}", efi);
    let s = Command::new("mkfs.fat")
        .args(["-F", "32", &efi])
        .status()?;
    if !s.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("mkfs.fat başarısız: {}", efi),
        ));
    }

    // 2. SWAP (opsiyonel)
    if swap_enabled {
        let swap = part_path(device, part_num(2, swap_enabled));
        println!("Swap bölümü oluşturuluyor: {}", swap);
        let s = Command::new("mkswap").arg(&swap).status()?;
        if !s.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("mkswap başarısız: {}", swap),
            ));
        }
    }

    // 3. ROOT (ext4)
    let root = part_path(device, part_num(3, swap_enabled));
    println!("Root bölümü formatlanıyor: {}", root);
    let s = Command::new("mkfs.ext4").arg(&root).status()?;
    if !s.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("mkfs.ext4 başarısız: {}", root),
        ));
    }

    println!("✅ Formatlama tamamlandı.");
    Ok(())
}

fn mount_system(device: &str, swap_enabled: bool) -> std::io::Result<()> {
    // Root bağla
    let root = part_path(device, part_num(3, swap_enabled));
    println!("Root bağlanıyor: {} → /mnt", root);
    let s = Command::new("mount").arg(&root).arg("/mnt").status()?;
    if !s.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Root mount başarısız!",
        ));
    }

    // EFI bağla
    let efi = part_path(device, part_num(1, swap_enabled));
    std::fs::create_dir_all("/mnt/boot")?;
    println!("EFI bağlanıyor: {} → /mnt/boot", efi);
    let s = Command::new("mount").arg(&efi).arg("/mnt/boot").status()?;
    if !s.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "EFI mount başarısız!",
        ));
    }

    // Swap etkinleştir (opsiyonel)
    if swap_enabled {
        let swap = part_path(device, part_num(2, swap_enabled));
        println!("Swap etkinleştiriliyor: {}", swap);
        let s = Command::new("swapon").arg(&swap).status()?;
        if !s.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "swapon başarısız!",
            ));
        }
    }

    println!("✅ Bağlama işlemleri tamamlandı.");
    Ok(())
}