use telemutter::{FrameError, parse_frame};

#[test]
fn vector_x_reserved_bits_set_rejected() {
    let frame = [0x21, 0x7A, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05];
    let err = parse_frame(&frame, 2, 0).unwrap_err();
    assert_eq!(err, FrameError::ReservedBitsSet);
}

#[test]
fn vector_y_wrong_version_rejected() {
    let frame = [0x60, 0x7A, 0x40, 0x01, 0x02, 0x03, 0x04, 0x05];
    let err = parse_frame(&frame, 2, 0).unwrap_err();
    assert_eq!(err, FrameError::WrongProtocolVersion);
}

#[test]
fn vector_z_invalid_schema_lane_width_for_sid32_rejected() {
    let frame = [0x30, 0x01, 0x02, 0x03, 0x04, 0x40, 0xAA, 0xBB];
    let err = parse_frame(&frame, 2, 0).unwrap_err();
    assert_eq!(err, FrameError::InvalidSchemaLaneWidth);
}
