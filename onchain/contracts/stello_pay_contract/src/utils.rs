use soroban_sdk::{Address, Env, String};

pub const MAX_ID_LEN: usize = 512;

pub fn append_u64(mut n: u64, buf: &mut [u8; MAX_ID_LEN], off: &mut usize) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    if n == 0 {
        i -= 1;
        tmp[i] = b'0';
    } else {
        while n > 0 {
            i -= 1;
            tmp[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
    }
    let digits = &tmp[i..];
    buf[*off..*off + digits.len()].copy_from_slice(digits);
    *off += digits.len();
}
