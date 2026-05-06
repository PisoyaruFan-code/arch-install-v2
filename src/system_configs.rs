use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use inquire::{MultiSelect, Password, PasswordDisplayMode, Select, Text};

// ── Dinamik veri çekme ────────────────────────────────────────────────────────

pub fn get_available_locales() -> Vec<String> {
    let content = std::fs::read_to_string("/usr/share/i18n/SUPPORTED").unwrap_or_default();

    let mut locales: Vec<String> = content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            l.split_whitespace()
                .next()
                .unwrap_or(l)
                .to_string()
        })
        .collect();

    locales.sort();
    locales.dedup();
    locales
}

pub fn get_available_timezones() -> Vec<String> {
    if let Ok(out) = Command::new("timedatectl").arg("list-timezones").output() {
        if out.status.success() {
            let list: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if !list.is_empty() {
                return list;
            }
        }
    }

    let mut zones = Vec::new();
    scan_zoneinfo("/usr/share/zoneinfo", "", &mut zones);
    zones.sort();
    zones
}

fn scan_zoneinfo(base: &str, prefix: &str, result: &mut Vec<String>) {
    let dir_path = if prefix.is_empty() {
        base.to_string()
    } else {
        format!("{}/{}", base, prefix)
    };

    let Ok(entries) = std::fs::read_dir(&dir_path) else {
        return;
    };

    const SKIP: &[&str] = &[
        "posix", "right", "leap-seconds.list", "+VERSION", "SECURITY",
        "localtime", "zone.tab", "zone1970.tab", "iso3166.tab",
    ];

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        if SKIP.contains(&name.as_str()) {
            continue;
        }
        if name.starts_with(|c: char| c.is_ascii_lowercase()) {
            continue;
        }

        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };

        match entry.file_type() {
            Ok(ft) if ft.is_dir() => {
                scan_zoneinfo(base, &relative, result);
            }
            Ok(_) => {
                if relative.contains('/') {
                    result.push(relative);
                }
            }
            Err(_) => {}
        }
    }
}

// ── Veri yapıları ─────────────────────────────────────────────────────────────

pub struct SystemConfig {
    pub hostname: String,
    pub locales: Vec<String>,
    pub timezone: String,
    pub root_password: String,
}

pub struct UserConfig {
    pub username: String,
    pub password: String,
    pub groups: Vec<String>,
}

// ── Sistem yapılandırması toplama ─────────────────────────────────────────────

pub fn gather_system_config() -> SystemConfig {
    println!("\n⚙️  Sistem yapılandırması başlıyor...\n");

    println!("📦 Locale listesi yükleniyor...");
    let all_locales = get_available_locales();
    if all_locales.is_empty() {
        eprintln!("⚠️  /usr/share/i18n/SUPPORTED okunamadı; locale listesi boş!");
    }

    println!("🌍 Timezone listesi yükleniyor...");
    let all_timezones = get_available_timezones();
    if all_timezones.is_empty() {
        eprintln!("⚠️  Timezone listesi boş! timedatectl veya /usr/share/zoneinfo kontrol edin.");
    }

    let default_locale_idxs: Vec<usize> = ["tr_TR.UTF-8", "en_US.UTF-8"]
        .iter()
        .filter_map(|target| all_locales.iter().position(|l| l == target))
        .collect();

    let default_tz_idx = all_timezones
        .iter()
        .position(|t| t == "Europe/Istanbul")
        .unwrap_or(0);

    println!("✅ Hazır. Yapılandırma sorularına geçiliyor...\n");

    // ── 1. Hostname ──────────────────────────────────────────────────────────
    let hostname = loop {
        let input = Text::new("Bilgisayar adı (hostname):")
            .with_default("archlinux")
            .with_help_message("Sadece harf, rakam ve tire. Örn: my-arch")
            .prompt()
            .expect("Hostname okunamadı");

        let trimmed = input.trim().to_string();

        if trimmed.is_empty() {
            println!("⚠️  Hostname boş bırakılamaz!");
            continue;
        }
        if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            println!("⚠️  Geçersiz karakter! Sadece harf, rakam ve tire kullanın.");
            continue;
        }
        if trimmed.starts_with('-') || trimmed.ends_with('-') {
            println!("⚠️  Hostname tire ile başlayamaz veya bitemez!");
            continue;
        }
        if trimmed.len() > 63 {
            println!("⚠️  Hostname en fazla 63 karakter olabilir!");
            continue;
        }

        break trimmed;
    };

    // ── 2. Locale (çoklu seçim) ──────────────────────────────────────────────
    let locales = loop {
        let selected = MultiSelect::new(
            "Locale(ler) seçin (Boşluk ile işaretle, Enter ile onayla):",
            all_locales.clone(),
        )
        .with_default(&default_locale_idxs)
        .with_help_message(
            "En az 1 seçmelisiniz. İlk seçilen birincil (LANG=) locale olur. Arama için yazın.",
        )
        .prompt()
        .expect("Locale seçimi okunamadı");

        if selected.is_empty() {
            println!("⚠️  En az bir locale seçmelisiniz!");
            continue;
        }
        break selected;
    };

    // ── 3. Timezone ──────────────────────────────────────────────────────────
    let timezone = Select::new("Saat dilimi seçin:", all_timezones)
        .with_starting_cursor(default_tz_idx)
        .with_help_message("↑↓ hareket, Enter seç. Arama için yazmaya başlayın.")
        .prompt()
        .expect("Timezone seçimi okunamadı");

    // ── 4. Root şifresi ──────────────────────────────────────────────────────
    let root_password = loop {
        let pass = Password::new("Root şifresi:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .with_help_message("En az 4 karakter")
            .without_confirmation()
            .prompt()
            .expect("Şifre okunamadı");

        if pass.len() < 4 {
            println!("⚠️  Şifre en az 4 karakter olmalı!");
            continue;
        }

        let confirm = Password::new("Root şifresi (tekrar):")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()
            .expect("Şifre tekrarı okunamadı");

        if pass != confirm {
            println!("⚠️  Şifreler eşleşmiyor, tekrar deneyin.");
            continue;
        }
        break pass;
    };

    SystemConfig {
        hostname,
        locales,
        timezone,
        root_password,
    }
}

// ── Kullanıcı yapılandırması toplama ──────────────────────────────────────────

pub fn gather_user_config() -> UserConfig {
    println!("\n👤 Kullanıcı oluşturma adımı...\n");

    // ── Kullanıcı adı ────────────────────────────────────────────────────────
    let username = loop {
        let input = Text::new("Kullanıcı adı:")
            .with_help_message("Küçük harf, rakam, tire ve alt çizgi. Örn: ahmet")
            .prompt()
            .expect("Kullanıcı adı okunamadı");

        let trimmed = input.trim().to_string();

        if trimmed.is_empty() {
            println!("⚠️  Kullanıcı adı boş bırakılamaz!");
            continue;
        }
        // Linux kullanıcı adı kuralları: küçük harf veya '_' ile başlamalı
        if !trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_lowercase() || c == '_')
            .unwrap_or(false)
        {
            println!("⚠️  Kullanıcı adı küçük harf veya '_' ile başlamalı!");
            continue;
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
        {
            println!("⚠️  Geçersiz karakter! Sadece küçük harf, rakam, -, _ ve . kullanın.");
            continue;
        }
        if trimmed.len() > 32 {
            println!("⚠️  Kullanıcı adı en fazla 32 karakter olabilir!");
            continue;
        }

        break trimmed;
    };

    // ── Kullanıcı şifresi ────────────────────────────────────────────────────
    let password = loop {
        let pass = Password::new("Kullanıcı şifresi:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .with_help_message("En az 4 karakter")
            .without_confirmation()
            .prompt()
            .expect("Şifre okunamadı");

        if pass.len() < 4 {
            println!("⚠️  Şifre en az 4 karakter olmalı!");
            continue;
        }

        let confirm = Password::new("Kullanıcı şifresi (tekrar):")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()
            .expect("Şifre tekrarı okunamadı");

        if pass != confirm {
            println!("⚠️  Şifreler eşleşmiyor, tekrar deneyin.");
            continue;
        }
        break pass;
    };

    // ── Gruplar ──────────────────────────────────────────────────────────────
    let available_groups = vec![
        "wheel".to_string(),
        "audio".to_string(),
        "video".to_string(),
        "storage".to_string(),
        "optical".to_string(),
        "network".to_string(),
        "power".to_string(),
        "lp".to_string(),
        "scanner".to_string(),
    ];

    let default_group_idxs: Vec<usize> = ["wheel", "audio", "video", "storage"]
        .iter()
        .filter_map(|target| available_groups.iter().position(|g| g == target))
        .collect();

    let groups = MultiSelect::new(
        "Kullanıcı grupları (Boşluk ile işaretle, Enter ile onayla):",
        available_groups,
    )
    .with_default(&default_group_idxs)
    .with_help_message("wheel = sudo erişimi sağlar")
    .prompt()
    .expect("Grup seçimi okunamadı");

    UserConfig {
        username,
        password,
        groups,
    }
}

// ── fstab ─────────────────────────────────────────────────────────────────────

pub fn generate_fstab() -> std::io::Result<()> {
    println!("📄 fstab oluşturuluyor...");

    let out = Command::new("genfstab").args(["-U", "/mnt"]).output()?;

    if !out.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("genfstab başarısız: {}", String::from_utf8_lossy(&out.stderr)),
        ));
    }

    std::fs::create_dir_all("/mnt/etc")?;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("/mnt/etc/fstab")?;

    file.write_all(&out.stdout)?;
    println!("✅ fstab yazıldı.");
    Ok(())
}

// ── Sistem yapılandırması uygulama ────────────────────────────────────────────

pub fn apply_system_config(cfg: &SystemConfig, optional_packages: &[String]) -> std::io::Result<()> {
    set_timezone(&cfg.timezone)?;
    set_locale(&cfg.locales)?;
    set_hostname(&cfg.hostname)?;
    set_root_password(&cfg.root_password)?;
    configure_mkinitcpio(optional_packages)?;
    install_grub()?;
    Ok(())
}

fn set_timezone(timezone: &str) -> std::io::Result<()> {
    println!("🕐 Saat dilimi ayarlanıyor: {}", timezone);

    let zoneinfo = format!("/usr/share/zoneinfo/{}", timezone);

    if !Path::new(&zoneinfo).exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Zoneinfo dosyası bulunamadı: {}", zoneinfo),
        ));
    }

    arch_chroot(&["ln", "-sf", &zoneinfo, "/etc/localtime"])?;
    arch_chroot(&["hwclock", "--systohc"])?;

    println!("✅ Saat dilimi ayarlandı.");
    Ok(())
}

fn set_locale(locales: &[String]) -> std::io::Result<()> {
    println!("🌐 Locale ayarlanıyor: {}", locales.join(", "));

    let locale_gen_path = "/mnt/etc/locale.gen";
    let content = std::fs::read_to_string(locale_gen_path).unwrap_or_default();

    let updated: String = content
        .lines()
        .map(|line| {
            let stripped = line.trim_start_matches('#').trim_start();
            if locales.iter().any(|l| stripped.starts_with(l.as_str())) {
                stripped.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(locale_gen_path, format!("{}\n", updated))?;
    arch_chroot(&["locale-gen"])?;

    let primary = &locales[0];
    std::fs::write("/mnt/etc/locale.conf", format!("LANG={}\n", primary))?;

    println!("✅ Locale ayarlandı. Birincil: {}", primary);
    Ok(())
}

fn set_hostname(hostname: &str) -> std::io::Result<()> {
    println!("💻 Hostname ayarlanıyor: {}", hostname);

    std::fs::write("/mnt/etc/hostname", format!("{}\n", hostname))?;

    let hosts = format!(
        "127.0.0.1\tlocalhost\n\
         ::1\t\tlocalhost\n\
         127.0.1.1\t{hostname}.localdomain {hostname}\n"
    );
    std::fs::write("/mnt/etc/hosts", hosts)?;

    println!("✅ Hostname ayarlandı.");
    Ok(())
}

fn set_root_password(password: &str) -> std::io::Result<()> {
    println!("🔑 Root şifresi ayarlanıyor...");

    let mut child = Command::new("arch-chroot")
        .args(["/mnt", "chpasswd"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(format!("root:{}\n", password).as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "chpasswd başarısız!",
        ));
    }

    println!("✅ Root şifresi ayarlandı.");
    Ok(())
}

// ── Kullanıcı oluşturma ───────────────────────────────────────────────────────

pub fn create_user(cfg: &UserConfig) -> std::io::Result<()> {
    println!("👤 Kullanıcı oluşturuluyor: {}", cfg.username);

    let groups_str = cfg.groups.join(",");

    // useradd: ev dizini oluştur (-m), bash kabuğu ata (-s), gruplara ekle (-G)
    arch_chroot(&[
        "useradd",
        "-m",
        "-s", "/bin/bash",
        "-G", &groups_str,
        &cfg.username,
    ])?;

    // Kullanıcı şifresini ayarla
    let mut child = Command::new("arch-chroot")
        .args(["/mnt", "chpasswd"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(format!("{}:{}\n", cfg.username, cfg.password).as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Kullanıcı şifresi ayarlanamadı (chpasswd başarısız)!",
        ));
    }

    println!("✅ Kullanıcı '{}' oluşturuldu.", cfg.username);

    // Kullanıcı wheel grubundaysa sudoers'ı yapılandır
    if cfg.groups.iter().any(|g| g == "wheel") {
        configure_sudoers_wheel()?;
    }

    Ok(())
}

/// /mnt/etc/sudoers dosyasında "%wheel ALL=(ALL:ALL) ALL" satırını
/// yorum satırından çıkarır. sudo yüklü değilse sessizce atlar.
fn configure_sudoers_wheel() -> std::io::Result<()> {
    let sudoers_path = "/mnt/etc/sudoers";

    if !Path::new(sudoers_path).exists() {
        println!("ℹ️  sudo kurulu değil, sudoers yapılandırması atlandı.");
        return Ok(());
    }

    println!("🔓 sudoers wheel grubu etkinleştiriliyor...");

    let content = std::fs::read_to_string(sudoers_path)?;

    // "# %wheel ALL=(ALL:ALL) ALL" ve "#%wheel ALL=(ALL:ALL) ALL"
    // biçimlerini yakalar, yorum karakterini kaldırır.
    let updated = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let without_comment = trimmed.trim_start_matches('#').trim_start();
                if without_comment == "%wheel ALL=(ALL:ALL) ALL" {
                    return "%wheel ALL=(ALL:ALL) ALL".to_string();
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    let updated = format!("{}\n", updated);

    // Geçici dosyaya yaz → visudo -c ile doğrula → başarılıysa yerleştir.
    let tmp_path = "/mnt/etc/sudoers.tmp";
    std::fs::write(tmp_path, &updated)?;

    let check = Command::new("arch-chroot")
        .args(["/mnt", "visudo", "-c", "-f", "/etc/sudoers.tmp"])
        .status();

    match check {
        Ok(s) if s.success() => {
            std::fs::rename(tmp_path, sudoers_path)?;
            println!("✅ sudoers: wheel grubu için sudo erişimi etkinleştirildi.");
        }
        _ => {
            let _ = std::fs::remove_file(tmp_path);
            eprintln!(
                "⚠️  sudoers doğrulaması başarısız! Dosya değiştirilmedi. \
                 Kurulum sonrası 'visudo' ile manuel olarak düzenleyin."
            );
        }
    }

    Ok(())
}

fn configure_mkinitcpio(optional_packages: &[String]) -> std::io::Result<()> {
    println!("🔧 mkinitcpio.conf yapılandırılıyor...");

    let mkinitcpio_path = "/mnt/etc/mkinitcpio.conf";

    if !Path::new(mkinitcpio_path).exists() {
        println!("ℹ️  mkinitcpio.conf bulunamadı, atlandı.");
        return Ok(());
    }

    let content = std::fs::read_to_string(mkinitcpio_path)?;

    let mut modules_to_add = Vec::new();
    if optional_packages.contains(&"amd-ucode".to_string()) {
        modules_to_add.push("amd-ucode");
    }
    if optional_packages.contains(&"intel-ucode".to_string()) {
        modules_to_add.push("intel-ucode");
    }

    if modules_to_add.is_empty() {
        println!("ℹ️  Ek MODULES gerekmiyor.");
        return Ok(());
    }

    let updated = content
        .lines()
        .map(|line| {
            if line.trim().starts_with("MODULES=") {
                let existing = line.trim_start_matches("MODULES=").trim();
                let (inner, format_type) = if existing.starts_with('(') && existing.ends_with(')') {
                    (&existing[1..existing.len()-1], "parens")
                } else if existing.starts_with('"') && existing.ends_with('"') {
                    (&existing[1..existing.len()-1], "quotes")
                } else {
                    (existing, "plain")
                };
                let mut new_modules = inner.split_whitespace().filter(|s| !s.is_empty()).collect::<Vec<_>>();
                for module in &modules_to_add {
                    if !new_modules.contains(module) {
                        new_modules.push(module);
                    }
                }
                match format_type {
                    "parens" => format!("MODULES=({})", new_modules.join(" ")),
                    "quotes" => format!("MODULES=\"{}\"", new_modules.join(" ")),
                    _ => format!("MODULES={}", new_modules.join(" ")),
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(mkinitcpio_path, format!("{}\n", updated))?;

    // mkinitcpio'yu yeniden oluştur
    arch_chroot(&["mkinitcpio", "-P"])?;

    println!("✅ mkinitcpio.conf güncellendi ve initramfs yeniden oluşturuldu.");
    Ok(())
}

fn install_grub() -> std::io::Result<()> {
    println!("🥾 GRUB kuruluyor...");

    let is_efi = Path::new("/sys/firmware/efi").exists();

    if is_efi {
        println!("  → UEFI sistemi tespit edildi.");
        println!("  → grub-install çalıştırılıyor...");
        arch_chroot(&[
            "grub-install",
            "--target=x86_64-efi",
            "--efi-directory=/boot",
            "--bootloader-id=GRUB",
            "--recheck",
        ])?;
    } else {
        println!("  → BIOS/Legacy sistemi tespit edildi.");
        let device = find_root_disk()?;
        println!("  → Boot diski: {}", device);
        println!("  → grub-install çalıştırılıyor...");
        arch_chroot(&["grub-install", "--target=i386-pc", "--recheck", &device])?;
    }

    println!("  → grub-mkconfig çalıştırılıyor...");
    arch_chroot(&["grub-mkconfig", "-o", "/boot/grub/grub.cfg"])?;

    println!("✅ GRUB kurulumu tamamlandı.");
    Ok(())
}

fn find_root_disk() -> std::io::Result<String> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/mnt"])
        .output()?;

    let source = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if source.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "/mnt bağlı değil veya kaynak bulunamadı!",
        ));
    }

    let disk = if source.contains("nvme") || source.contains("mmcblk") {
        let without_digits = source.trim_end_matches(|c: char| c.is_ascii_digit());
        without_digits.trim_end_matches('p').to_string()
    } else {
        source.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()
    };

    Ok(disk)
}

// ── Yardımcı: arch-chroot ─────────────────────────────────────────────────────

fn arch_chroot(args: &[&str]) -> std::io::Result<()> {
    let mut cmd_args = vec!["/mnt"];
    cmd_args.extend_from_slice(args);

    let status = Command::new("arch-chroot").args(&cmd_args).status()?;

    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "arch-chroot komutu başarısız: arch-chroot /mnt {}",
                args.join(" ")
            ),
        ));
    }
    Ok(())
}

// ── Ana akış ─────────────────────────────────────────────────────────────────

pub fn post_install(optional_packages: &[String]) -> std::io::Result<()> {
    generate_fstab()?;

    let sys_cfg = gather_system_config();
    apply_system_config(&sys_cfg, optional_packages)?;

    let user_cfg = gather_user_config();
    create_user(&user_cfg)?;

    println!("\n🎉 Kurulum tamamlandı!");
    println!("   Sistemi yeniden başlatmak için:");
    println!("   → umount -R /mnt");
    println!("   → reboot");

    Ok(())
}