//! Tiny English/Turkish string table.
//!
//! The translations are copied from the original GTK application's locale
//! table (`locale_mgr.py`, `FALLBACK_DICT`) so both programs read the same.
//! Keys that only exist in this TUI are prefixed with `tui_`.
//!
//! The active language is process-wide state. Keys are looked up with [`t`];
//! a missing key is returned unchanged, so a forgotten translation shows the
//! key instead of an empty label.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// The languages we ship translations for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Tr,
}

impl Lang {
    /// Every language we ship, in the order the picker lists them.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Tr];

    /// ISO code, also used in the config file.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Tr => "tr",
        }
    }

    /// Name of the language in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Tr => "Türkçe",
        }
    }

    fn from_code(code: &str) -> Option<Lang> {
        match code.to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "tr" => Some(Lang::Tr),
            _ => None,
        }
    }
}

/// key -> (English, Turkish)
static TABLE: LazyLock<HashMap<&'static str, (&'static str, &'static str)>> = LazyLock::new(|| {
    TRANSLATIONS
        .iter()
        .map(|(key, en, tr)| (*key, (*en, *tr)))
        .collect()
});

/// The active language, defaulting to English until [`init`] runs.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

/// The language currently in use.
pub fn lang() -> Lang {
    if ACTIVE.load(Ordering::Relaxed) == 1 {
        Lang::Tr
    } else {
        Lang::En
    }
}

/// Switch the language (does not persist it, see [`save_preference`]).
pub fn set_lang(lang: Lang) {
    ACTIVE.store(u8::from(lang == Lang::Tr), Ordering::Relaxed);
}

/// Translation of `key` in a given language; unknown keys map to themselves.
pub fn translation(lang: Lang, key: &str) -> &str {
    match TABLE.get(key) {
        Some(&(en, tr)) => match lang {
            Lang::En => en,
            Lang::Tr => tr,
        },
        None => key,
    }
}

/// Translation of `key` in the active language.
pub fn t(key: &str) -> &str {
    translation(lang(), key)
}

/// Fill the `{}` placeholders of a translated template, left to right.
///
/// `format!` needs a literal template, so translated templates are filled at
/// runtime with this helper instead.
pub fn fill(template: &str, values: &[&str]) -> String {
    let mut text = template.to_string();
    for value in values {
        text = text.replacen("{}", value, 1);
    }
    text
}

/// Switch to `lang` and remember the choice.
pub fn choose(lang: Lang) {
    set_lang(lang);
    save_preference(lang);
}

/// Load the saved language, falling back to the system locale.
pub fn init() {
    set_lang(saved_language().unwrap_or_else(detect_language));
}

fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/linux-boot-analyzer-tui/config.json")
}

/// The language saved by a previous run, if any.
fn saved_language() -> Option<Lang> {
    let contents = fs::read_to_string(config_path()).ok()?;
    let json: serde_json::Value = serde_json::from_str(&contents).ok()?;
    Lang::from_code(json.get("lang")?.as_str()?)
}

/// Guess the language from `LANG`/`LC_ALL`, defaulting to English.
fn detect_language() -> Lang {
    let value = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    let code = value.split(['.', '_']).next().unwrap_or_default();
    Lang::from_code(code).unwrap_or(Lang::En)
}

/// Remember the language choice for the next run.
pub fn save_preference(lang: Lang) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, format!("{{\n  \"lang\": \"{}\"\n}}\n", lang.code()));
}

/// The string table: (key, English, Turkish).
const TRANSLATIONS: &[(&str, &str, &str)] = &[
    // --- Tab titles -------------------------------------------------------
    ("tab_analysis", "Analysis", "Analiz"),
    ("tab_services", "Services", "Servisler"),
    ("tab_autostart", "Autostart", "Başlangıç"),
    ("tab_profiles", "Profiles", "Profiller"),
    ("tab_backups", "Backups", "Yedekler"),
    // --- Services unit types ----------------------------------------------
    (
        "view_services",
        "Services (.service)",
        "Servisler (.service)",
    ),
    (
        "view_others",
        "Others (socket, target, mount)",
        "Diğerleri (socket, target, mount)",
    ),
    ("view_devices", "Devices (.device)", "Aygıtlar (.device)"),
    ("tui_view_count", "{}/{} {}", "{}/{} {}"),
    (
        "tui_device_not_manageable",
        "Device units cannot be managed from here.",
        "Aygıt birimleri buradan yönetilemez.",
    ),
    ("sub_state", "Sub State", "Alt Durum"),
    // --- Analysis page ----------------------------------------------------
    ("tui_system", "System", "Sistem"),
    ("sys_os", "OS:", "Sistem:"),
    ("sys_kernel", "Kernel:", "Çekirdek:"),
    ("sys_ram", "RAM:", "Bellek:"),
    ("sys_uptime", "Uptime:", "Çalışma:"),
    ("boot_suresi", "Boot Time", "Açılış Süresi"),
    ("toplam_boot", "Total Boot Time", "Toplam Açılış Süresi"),
    ("comp_firmware", "Firmware", "Donanım (Firmware)"),
    ("comp_loader", "Loader", "Önyükleyici (Loader)"),
    ("comp_kernel", "Kernel", "Çekirdek (Kernel)"),
    (
        "comp_initrd",
        "Initrd (Early Boot)",
        "Başlangıç Arayüzü (Initrd)",
    ),
    ("comp_userspace", "Userspace", "Kullanıcı Alanı (Userspace)"),
    (
        "startup_optimizasyonu",
        "Boot Optimization",
        "Başlangıç Optimizasyonu",
    ),
    (
        "no_optimizable_services",
        "No recommended service to disable; the system is in an optimal state!",
        "Kapatılması önerilen bir hizmet yok; sistem en iyi durumda!",
    ),
    (
        "tui_slowest_services",
        "Slowest Services",
        "En Yavaş Servisler",
    ),
    (
        "tui_optimize_hint",
        "Press 'o' to disable every recommended service.",
        "Tüm önerilen hizmetleri kapatmak için 'o' tuşuna basın.",
    ),
    (
        "tui_suggested_services",
        "Recommended services:",
        "Önerilen hizmetler:",
    ),
    ("tui_second", "s", "sn"),
    // --- Services page ----------------------------------------------------
    ("service_name", "Service", "Hizmet"),
    ("filter_active", "Active", "Aktif"),
    ("tui_boot_config", "Enabled", "Etkin"),
    ("time", "Time", "Süre"),
    ("tui_tip", "Tip", "Tür"),
    ("tui_details", "Details", "Detaylar"),
    ("tui_status", "status", "durum"),
    ("tui_tip_filter", "tip", "tür"),
    ("tui_sort", "sort", "sıralama"),
    ("tui_search", "search", "ara"),
    ("tui_sort_name", "name", "ad"),
    ("tui_sort_time", "boot time", "açılış süresi"),
    ("tip_all", "All Categories", "Tüm Kategoriler"),
    ("tip_suggestion", "Can Be Disabled", "Kapatılabilir"),
    ("tip_critical", "Do Not Disable", "Kapatılmamalı"),
    ("tip_required", "Required", "Gerekli"),
    ("filter_all", "All States", "Tüm Durumlar"),
    ("filter_inactive", "Inactive", "Pasif"),
    ("filter_disabled", "Disabled", "Devre Dışı"),
    ("filter_masked", "Masked", "Maskeli"),
    ("tui_tip_suggestion", "Suggestion", "Öneri"),
    ("tui_tip_critical", "Critical", "Kritik"),
    ("tui_tip_required", "Required", "Gerekli"),
    ("tui_state_enabled", "enabled", "etkin"),
    ("tui_state_disabled", "disabled", "devre dışı"),
    ("tui_state_failed", "failed", "başarısız"),
    (
        "tui_no_kb_entry",
        "No knowledge-base entry for this service.",
        "Bu hizmet için bilgi tabanı kaydı bulunamadı.",
    ),
    (
        "tui_no_service_selected",
        "No service selected.",
        "Servis seçilmedi.",
    ),
    // --- Actions ----------------------------------------------------------
    ("tui_action_enable", "Enable", "Etkinleştir"),
    ("tui_action_disable", "Disable", "Kapat"),
    ("tui_action_start", "Start", "Başlat"),
    ("tui_action_stop", "Stop", "Durdur"),
    ("tui_action_mask", "Mask", "Maskele"),
    ("tui_action_unmask", "Unmask", "Maske kaldır"),
    (
        "tui_prompt_enable",
        "Enable {} at boot?",
        "{} açılışta etkinleştirilsin mi?",
    ),
    (
        "tui_prompt_disable",
        "Disable {} at boot?",
        "{} açılışta kapatılsın mı?",
    ),
    (
        "tui_prompt_start",
        "Start {} now?",
        "{} şimdi başlatılsın mı?",
    ),
    (
        "tui_prompt_stop",
        "Stop {} now?",
        "{} şimdi durdurulsun mu?",
    ),
    ("tui_prompt_mask", "Mask {}?", "{} maskelensin mi?"),
    (
        "tui_prompt_unmask",
        "Unmask {}?",
        "{} maskesi kaldırılsın mı?",
    ),
    (
        "tui_prompt_bulk",
        "Disable {} recommended service(s)?",
        "{} önerilen hizmet kapatılsın mı?",
    ),
    ("tui_prompt_confirm", "Confirm?", "Onayla?"),
    // --- Autostart page ---------------------------------------------------
    ("state", "State", "Durum"),
    ("delay", "Delay", "Gecikme"),
    ("komut", "Command", "Komut"),
    ("tui_enabled", "enabled", "etkin"),
    ("tui_disabled", "disabled", "kapalı"),
    ("tui_file", "File", "Dosya"),
    (
        "tui_new_autostart",
        "New Autostart Entry",
        "Yeni Otomatik Başlatma Kaydı",
    ),
    (
        "no_autostart_apps",
        "No startup application found. Press 'n' to add one.",
        "Otomatik başlatılan uygulama yok. Eklemek için 'n' tuşuna basın.",
    ),
    // --- Profiles and backups --------------------------------------------
    (
        "profiller_title",
        "System Boot Profiles",
        "Sistem Başlangıç Profilleri",
    ),
    (
        "backup_noktalari",
        "Restore Points",
        "Geri Yükleme Noktaları",
    ),
    ("tui_profile", "Profile", "Profil"),
    ("tui_enable_count", "Enable", "Etkinleştir"),
    ("tui_disable_count", "Disable", "Kapat"),
    ("service_count", "services", "hizmet"),
    (
        "tui_create_backup_hint",
        "No restore point yet. Press 'b' to create one.",
        "Henüz geri yükleme noktası yok. Oluşturmak için 'b' tuşuna basın.",
    ),
    ("tui_plan_enable", "enable", "etkinleştir"),
    ("tui_plan_start", "start", "başlat"),
    ("tui_plan_disable", "disable", "kapat"),
    ("tui_plan_stop", "stop", "durdur"),
    (
        "user_custom_profilleri",
        "User Custom Profiles",
        "Kullanıcı Özel Profilleri",
    ),
    (
        "no_custom_profiles",
        "No custom profiles found.",
        "Kayıtlı özel profil bulunamadı.",
    ),
    (
        "yeni_custom_profile_btn",
        "Create New Custom Profile",
        "Yeni Özel Profil Oluştur",
    ),
    ("custom_profilim", "My Custom Profile", "Özel Profilim"),
    ("service_kurali", "service rules", "hizmet kuralı"),
    ("profili_save_title", "Save Profile", "Profili Kaydet"),
    (
        "yeni_profile_name_girin",
        "Enter new profile name:",
        "Yeni profil adı girin:",
    ),
    ("profile_name_lbl", "Profile Name", "Profil Adı"),
    (
        "profile_name_girin_error",
        "Please enter a profile name.",
        "Lütfen profil adı girin.",
    ),
    (
        "service_kurallari_lbl",
        "Service Rules:",
        "Hizmet Kuralları:",
    ),
    (
        "service_kurallari_sub",
        "On = Enabled, Off = Disabled",
        "Açık = Etkinleştirilir, Kapalı = Devre dışı bırakılır",
    ),
    (
        "profili_kaydedildi",
        "profile saved.",
        "profili kaydedildi.",
    ),
    (
        "custom_profile_olusturuldu",
        "Custom profile '{}' successfully created.",
        "Özel profil '{}' başarıyla oluşturuldu.",
    ),
    (
        "custom_profile_deleted",
        "Custom profile deleted.",
        "Özel profil silindi.",
    ),
    // --- Overlays ---------------------------------------------------------
    ("tui_confirm", "Confirm", "Onayla"),
    (
        "tui_confirm_keys",
        "y / Enter to confirm, n / Esc to cancel",
        "Onaylamak için y / Enter, iptal için n / Esc",
    ),
    (
        "tui_password_keys",
        "Enter to run, Esc to cancel",
        "Çalıştırmak için Enter, iptal için Esc",
    ),
    ("tui_delay_prompt", "Delay in seconds:", "Gecikme (saniye):"),
    (
        "tui_delay_keys",
        "↑/↓: 1s    Enter: save    Esc: cancel",
        "↑/↓: 1sn    Enter: kaydet    Esc: iptal",
    ),
    (
        "tui_delay_invalid",
        "Enter the delay as a whole number of seconds.",
        "Gecikmeyi saniye cinsinden tam sayı olarak girin.",
    ),
    // --- Status messages --------------------------------------------------
    ("tui_loading", "Loading...", "Yükleniyor..."),
    ("tui_no_data", "No data available.", "Veri yok."),
    ("tui_running", "Running...", "Çalışıyor..."),
    (
        "tui_busy",
        "A command is already running.",
        "Bir komut zaten çalışıyor.",
    ),
    ("tui_done", "Done.", "Tamamlandı."),
    (
        "tui_nothing_to_optimize",
        "Nothing to optimize.",
        "Optimize edilecek bir şey yok.",
    ),
    (
        "tui_report_copied",
        "Report written to {} and copied to the clipboard",
        "Rapor şuraya yazıldı ve panoya kopyalandı: {}",
    ),
    (
        "tui_report_failed",
        "Could not write {}: {}",
        "{} yazılamadı: {}",
    ),
    (
        "tui_profile_matches",
        "The profile already matches the system.",
        "Profil zaten sistemle uyumlu.",
    ),
    (
        "tui_backup_matches",
        "The system already matches this backup.",
        "Sistem zaten bu yedekle uyumlu.",
    ),
    (
        "tui_autostart_updated",
        "Autostart entry updated.",
        "Otomatik başlatma kaydı güncellendi.",
    ),
    (
        "tui_autostart_removed",
        "Autostart entry removed.",
        "Otomatik başlatma kaydı kaldırıldı.",
    ),
    ("tui_backup_deleted", "Backup deleted.", "Yedek silindi."),
    ("tui_created", "Created {}", "Oluşturuldu: {}"),
    ("tui_saved_backup", "Saved {}", "Kaydedildi: {}"),
    (
        "tui_name_required",
        "A name is required.",
        "Bir isim gerekli.",
    ),
    (
        "tui_command_required",
        "A command is required.",
        "Bir komut gerekli.",
    ),
    ("tui_language_title", "Language", "Dil"),
    ("tui_language_set", "Language: {}", "Dil: {}"),
    // --- Report -----------------------------------------------------------
    (
        "tui_report_title",
        "Linux Boot Analysis Report",
        "Linux Açılış Analizi Raporu",
    ),
    (
        "tui_report_footer",
        "Generated by linux_boot_analyzer_tui",
        "linux_boot_analyzer_tui tarafından oluşturuldu",
    ),
    // --- Footer: contextual action bar ------------------------------------
    (
        "tui_bar_menu",
        "Enter: choose    Up/Down: move    Esc: cancel",
        "Enter: seç    Yukarı/Aşağı: gez    Esc: iptal",
    ),
    (
        "tui_bar_language",
        "Up/Down: choose    Enter: apply    Esc: cancel",
        "Yukarı/Aşağı: seç    Enter: uygula    Esc: iptal",
    ),
    (
        "tui_bar_confirm",
        "y: confirm    n or Esc: cancel",
        "y: onayla    n veya Esc: iptal",
    ),
    (
        "tui_bar_password",
        "Enter: run    Esc: cancel",
        "Enter: çalıştır    Esc: iptal",
    ),
    (
        "tui_bar_form",
        "Tab: next field    Enter: create    Esc: cancel",
        "Tab: sonraki alan    Enter: oluştur    Esc: iptal",
    ),
    (
        "tui_bar_profile_form",
        "Tab/arrows: move    space: on/off    Enter: create    Esc: cancel",
        "Tab/oklar: gez    boşluk: aç/kapat    Enter: oluştur    Esc: iptal",
    ),
    (
        "tui_bar_profile_name",
        "Enter: save    Esc: cancel",
        "Enter: kaydet    Esc: iptal",
    ),
    (
        "tui_bar_delay",
        "digits or Up/Down: seconds    Enter: save    Esc: cancel",
        "rakam veya Yukarı/Aşağı: saniye    Enter: kaydet    Esc: iptal",
    ),
    (
        "tui_bar_info",
        "Up/Down or j/k: scroll    PageUp/PageDown: page    Esc: close",
        "Yukarı/Aşağı veya j/k: kaydır    PgUp/PgDn: sayfa    Esc: kapat",
    ),
    (
        "tui_bar_analysis",
        "o: optimize    w: report",
        "o: en iyile    w: rapor yaz",
    ),
    (
        "tui_bar_services",
        "Enter: actions    v: unit type    d/D: dependencies    l: journal",
        "Enter: işlemler    v: birim türü    d/D: bağımlılıklar    l: günlük",
    ),
    (
        "tui_bar_autostart",
        "space: enable/disable    d: delay    n: add    x: remove",
        "boşluk: aç/kapat    d: gecikme    n: ekle    x: sil",
    ),
    (
        "tui_bar_profiles",
        "Enter: apply    n: new    w: save state    x: delete    b: restore point",
        "Enter: uygula    n: yeni    w: durum kaydet    x: sil    b: geri yükleme noktası",
    ),
    (
        "tui_bar_backups",
        "d: restore    x: delete    b: restore point",
        "d: geri yükle    x: sil    b: nokta oluştur",
    ),
    (
        "tui_bar_global",
        "q: quit    Tab: tab    ↑/↓: move    ?: help    L: language",
        "q: çık    Tab: sekme    ↑/↓: gez    ?: yardım    L: dil",
    ),
    // --- Help overlay -----------------------------------------------------
    ("tui_help_navigation", "Navigation", "Gezinme"),
    (
        "tui_help_quit",
        "  q / Esc          quit",
        "  q / Esc          çıkış",
    ),
    (
        "tui_help_tab",
        "  Tab / Shift+Tab  switch tab",
        "  Tab / Shift+Tab  sekme değiştir",
    ),
    (
        "tui_help_tab_keys",
        "  1 - 5            jump straight to a tab",
        "  1 - 5            doğrudan o sekmeye git",
    ),
    (
        "tui_help_move",
        "  Up / Down        move in a list, scroll a page",
        "  Yukarı / Aşağı   listede gez, sayfayı kaydır",
    ),
    (
        "tui_help_page",
        "  PageUp/PageDown  scroll a page quickly",
        "  PgUp/PgDn        sayfayı hızlı kaydır",
    ),
    (
        "tui_help_language",
        "  L                choose the language",
        "  L                dili seç",
    ),
    ("tui_help_services", "Services", "Servisler"),
    (
        "tui_help_view",
        "  v or Left/Right  unit type: services, others, devices",
        "  v veya Sol/Sağ    birim türü: servisler, diğerleri, aygıtlar",
    ),
    (
        "tui_help_search",
        "  /                search name and description",
        "  /                ad ve açıklamada ara",
    ),
    (
        "tui_help_filters",
        "  f / t / s        cycle status / tip / sort filters",
        "  f / t / s        durum / tür / sıralama filtresi",
    ),
    (
        "tui_help_actions",
        "  Enter or a       action menu (enable, disable, start, stop, mask)",
        "  Enter veya a     işlem menüsü (etkinleştir, kapat, başlat, durdur, maskele)",
    ),
    (
        "tui_help_log",
        "  l                journal log for the selected unit",
        "  l                seçili birimin günlüğü",
    ),
    (
        "tui_help_deps",
        "  d / D            dependencies / reverse dependencies",
        "  d / D            bağımlılıklar / ters bağımlılıklar",
    ),
    ("tui_help_analysis", "Analysis", "Analiz"),
    (
        "tui_help_optimize",
        "  o                disable every recommended service",
        "  o                önerilen tüm hizmetleri kapat",
    ),
    (
        "tui_help_report",
        "  w                write a text report",
        "  w                metin raporu yaz",
    ),
    ("tui_help_autostart", "Autostart", "Başlangıç"),
    (
        "tui_help_toggle",
        "  space            enable or disable the entry",
        "  boşluk           kaydı aç veya kapat",
    ),
    (
        "tui_help_delay",
        "  d                set the delay (Up/Down steps a second)",
        "  d                gecikmeyi gir (Yukarı/Aşağı birer saniye)",
    ),
    (
        "tui_help_add_remove",
        "  n / x            add a new entry / remove selected",
        "  n / x            yeni kayıt ekle / seçileni sil",
    ),
    ("tui_help_profiles", "Profiles", "Profiller"),
    (
        "tui_help_apply",
        "  Enter or a       apply the selected profile",
        "  Enter veya a     seçili profili uygula",
    ),
    (
        "tui_help_custom_new",
        "  n                create a custom profile",
        "  n                özel profil oluştur",
    ),
    (
        "tui_help_custom_save",
        "  w                save the current state as a profile",
        "  w                mevcut durumu profil olarak kaydet",
    ),
    (
        "tui_help_custom_delete",
        "  x                delete the selected custom profile",
        "  x                seçili özel profili sil",
    ),
    ("tui_help_backups", "Backups", "Yedekler"),
    (
        "tui_help_backup",
        "  b                create a restore point",
        "  b                geri yükleme noktası oluştur",
    ),
    (
        "tui_help_restore",
        "  d / x            restore / delete the selected backup",
        "  d / x            seçili yedeği geri yükle / sil",
    ),
    (
        "tui_help_reload",
        "  r                reload system data",
        "  r                sistem verisini yenile",
    ),
    ("tui_name", "Name", "Ad"),
    ("tui_backup", "Backup", "Yedek"),
    ("tui_services", "Services", "Hizmetler"),
    ("tui_profile_label", "Profile: {}", "Profil: {}"),
    ("tui_saved", "Saved: {}", "Kayıt: {}"),
    (
        "tui_no_profile_selected",
        "No profile selected.",
        "Profil seçilmedi.",
    ),
    (
        "tui_no_entry_selected",
        "No entry selected.",
        "Kayıt seçilmedi.",
    ),
    (
        "tui_potential_savings",
        "Potential savings",
        "Potansiyel kazanç",
    ),
    (
        "tui_remove_autostart",
        "Remove autostart entry \"{}\"?",
        "\"{}\" otomatik başlatma kaydı kaldırılsın mı?",
    ),
    (
        "tui_delete_backup_prompt",
        "Delete backup \"{}\"?",
        "\"{}\" yedeği silinsin mi?",
    ),
    (
        "tui_delete_profile_prompt",
        "Delete the custom profile \"{}\"?",
        "\"{}\" özel profili silinsin mi?",
    ),
    (
        "tui_apply_profile_prompt",
        "Apply this profile? ({})",
        "Bu profil uygulansın mı? ({})",
    ),
    (
        "tui_restore_backup_prompt",
        "Restore this backup? ({})",
        "Bu yedek noktasına dönülsün mü? ({})",
    ),
    // --- Info pane, overlays and titles ----------------------------------
    ("tui_help_title", "Help", "Yardım"),
    ("tui_authentication", "Authentication", "Kimlik Doğrulama"),
    ("tui_sudo_password", "sudo password:", "sudo şifresi:"),
    (
        "tui_info_hint",
        "Esc: close, j/k: scroll",
        "Esc: kapat, j/k: kaydır",
    ),
    ("tui_title_log", "Log: {}", "Günlük: {}"),
    (
        "tui_title_dependencies",
        "Dependencies: {}",
        "Bağımlılıklar: {}",
    ),
    (
        "tui_title_reverse_dependencies",
        "Reverse dependencies: {}",
        "Ters bağımlılıklar: {}",
    ),
    (
        "tui_disabled_count",
        "Disabled {} service(s).",
        "{} hizmet kapatıldı.",
    ),
    // --- Profile names and descriptions (same keys as the GTK app) --------
    ("prof_min_name", "Minimum Services", "Minimum Servis Modu"),
    (
        "prof_min_desc",
        "To boot the system as fast and lightweight as possible. Disables all additional services except network and basic system security. Saves laptop battery.",
        "Sistemi en hızlı ve hafif şekilde başlatmak için. Ağ ve temel sistem güvenliği dışındaki tüm ek servisler kapatılır. Pil tasarrufu sağlar.",
    ),
    ("prof_dev_name", "Developer Mode", "Yazılımcı Modu"),
    (
        "prof_dev_desc",
        "Designed for software developers. Docker, SSH, and database services are automatically enabled. Unnecessary services like printers are disabled.",
        "Yazılım geliştiriciler için hazırlandı. Docker, SSH ve veritabanı servisleri otomatik olarak etkinleştirilir. Yazıcı gibi gereksiz servisler kapatılır.",
    ),
    ("prof_office_name", "Office Mode", "Ofis Modu"),
    (
        "prof_office_desc",
        "Ideal settings for office tasks and daily use. Printer, Bluetooth, and network services remain enabled.",
        "Ofis işleri ve günlük kullanım için ideal ayarlar. Yazıcı, bluetooth ve ağ servisleri açık kalır.",
    ),
    ("tahmini_saving", "Estimated Savings", "Tahmini Kazanç"),
    // Friendly names used by the profile checklist (same keys as the GTK app).
    (
        "cups_service_desc",
        "Printer Service (CUPS)",
        "Yazıcı Servisi (CUPS)",
    ),
    ("bluetooth_desc", "Bluetooth Support", "Bluetooth Desteği"),
    (
        "docker_desc",
        "Docker Container Engine",
        "Docker Konteyner Motoru",
    ),
    (
        "postgresql_desc",
        "PostgreSQL Database",
        "PostgreSQL Veritabanı",
    ),
    ("ssh_desc", "SSH Remote Access", "SSH Uzaktan Erişim"),
    (
        "cups_browsed_desc",
        "Network Printer Discoverer",
        "Ağ Yazıcısı Bulucu",
    ),
    (
        "mysql_desc",
        "MySQL / MariaDB Database",
        "MySQL / MariaDB Veritabanı",
    ),
    (
        "avahi_desc",
        "Local Network Discovery (Avahi)",
        "Yerel Ağ Keşfi (Avahi)",
    ),
    (
        "modem_desc",
        "Cellular Modem Controller",
        "Hücresel Modem Kontrolü",
    ),
    ("unknown", "Unknown", "Bilinmiyor"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_both_languages_for_every_key() {
        assert!(!TRANSLATIONS.is_empty());
        for (key, en, tr) in TRANSLATIONS {
            assert!(!key.is_empty(), "empty key");
            assert!(!en.is_empty(), "missing English for {key}");
            assert!(!tr.is_empty(), "missing Turkish for {key}");
        }
    }

    #[test]
    fn duplicate_keys_are_rejected_by_the_map() {
        // Build the map again and make sure every literal key is unique.
        let mut seen = std::collections::HashSet::new();
        for (key, _, _) in TRANSLATIONS {
            assert!(seen.insert(*key), "duplicate key: {key}");
        }
    }

    #[test]
    fn translations_resolve_in_both_languages() {
        assert_eq!(translation(Lang::En, "tab_services"), "Services");
        assert_eq!(translation(Lang::Tr, "tab_services"), "Servisler");
        assert_eq!(
            translation(Lang::Tr, "prof_min_name"),
            "Minimum Servis Modu"
        );
    }

    #[test]
    fn unknown_keys_are_returned_unchanged() {
        assert_eq!(translation(Lang::Tr, "does_not_exist"), "does_not_exist");
    }

    #[test]
    fn every_language_is_listed_once() {
        // The picker offers exactly these, in this order.
        assert_eq!(Lang::ALL, [Lang::En, Lang::Tr]);
        assert_eq!(Lang::Tr.code(), "tr");
        assert_eq!(Lang::Tr.native_name(), "Türkçe");
    }

    #[test]
    fn detects_language_from_a_locale_name() {
        assert_eq!(Lang::from_code("tr_TR.UTF-8"), None); // full locale names are not codes
        assert_eq!(Lang::from_code("tr"), Some(Lang::Tr));
        assert_eq!(Lang::from_code("EN"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
    }
}
