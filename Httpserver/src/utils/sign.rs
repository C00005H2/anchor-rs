use md5;

use super::super::routes::user::LoginForm;

const AB_ENCRYPT_KEY: &str = "yXwBqDoXY3wTzo1DFf";

/// Very simplified: just append ABEncryptKey at the end of a fake concatenation
pub fn check_sign(form: &LoginForm) -> bool {
    // Example: just concat some fields + key
    let mut input = format!(
        "account={}&action=quick_login&game={}&loginType={}&password={}&platform={}&ts={}{}",
        form.account,
        form.game,
        form.loginType,
        form.password,
        form.platform,
        form.ts,
        AB_ENCRYPT_KEY
    );

    let digest = format!("{:x}", md5::compute(&input));
    println!("[SignCheck] input={} -> {}", input, digest);

    digest == form.sign
}
