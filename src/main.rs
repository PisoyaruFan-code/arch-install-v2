use inquire::Select;

mod disk_manager;
mod args;
mod packages;
mod system_configs;

fn main() {
    let disks = disk_manager::get_system_disks();

    if disks.is_empty() {
        println!("Hiç uygun disk bulunamadı!");
        return;
    }

    let selection = Select::new("Lütfen kurulum yapılacak diski seçin:", disks)
        .prompt();

    match selection {
        Ok(choice) => {
            // Seçilen string'den sadece "/dev/sda" kısmını ayıklayalım
            let disk_path = choice.split(' ').next().unwrap();
            println!("Seçilen hedef: {}", disk_path);
            
            disk_manager::format_the_disk(disk_path);

            if let Err(e) = packages::select_and_install_packages() {
                eprintln!("❌ Kurulum başarısız: {}", e);
            }

            if let Err(e) = system_configs::post_install() {
                eprintln!("❌ Kurulum sonrası işlemler başarısız: {}", e);
            }
        }
        Err(_) => println!("Seçim iptal edildi."),
    }
}