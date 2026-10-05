use super::super::routes::user::LoginForm;

const AB_ENCRYPT_KEY: &str = "yXwBqDoXY3wTzo1DFf";

/// Verify the login request signature. Keep the captured field ordering and
/// quick-login action for compatibility, but never log the password or digest
/// input.
pub fn check_sign(form: &LoginForm) -> bool {
    let input = format!(
        "account={}&action=quick_login&game={}&loginType={}&password={}&platform={}&ts={}{}",
        form.account,
        form.game,
        form.login_type,
        form.password,
        form.platform,
        form.ts,
        AB_ENCRYPT_KEY
    );
    let expected = format!("{:x}", md5::compute(input.as_bytes()));
    constant_time_eq(expected.as_bytes(), form.sign.as_bytes())
}

fn constant_time_eq(expected: &[u8], supplied: &[u8]) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    expected
        .iter()
        .zip(supplied)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_rejects_different_lengths_and_values() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }
}
