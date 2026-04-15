use crc_any::CRCu32;

pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = CRCu32::crc32();
    crc.digest(data);
    crc.get_crc()
}
