use proptest::prelude::*;

use super::*;

const BECH32_UPPER: &[u8] = b"QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L";

#[test]
fn short_id_vectors() {
    assert_eq!(ShortId([0; 8]).to_string(), "QQQQQQQQQQQQQ");
    // 64 one bits and one zero padding bit.
    assert_eq!(ShortId([0xff; 8]).to_string(), "LLLLLLLLLLLL7");
}

proptest! {
    #[test]
    fn short_id_is_13_uppercase_bech32_chars(bytes: [u8; 8]) {
        let id = ShortId(bytes).to_string();
        prop_assert_eq!(id.len(), 13);
        prop_assert!(id.bytes().all(|c| BECH32_UPPER.contains(&c)));
    }

    #[test]
    fn distinct_short_ids_display_distinctly(a: [u8; 8], b: [u8; 8]) {
        prop_assume!(a != b);
        prop_assert_ne!(ShortId(a).to_string(), ShortId(b).to_string());
    }

    #[test]
    fn get_and_post_address_the_same_path(bytes: [u8; 8], payload: Vec<u8>) {
        let id = ShortId(bytes);
        prop_assert_eq!(Get(id).path(), format!("/{id}"));
        prop_assert_eq!(Get(id).path(), Post { id, payload }.path());
    }

    #[test]
    fn statuses_not_defined_by_bip77_are_errors(status: u16, body: Vec<u8>) {
        if !matches!(status, 200 | 202) {
            prop_assert_eq!(Get::response(status, body.clone()), Err(UnexpectedStatus(status)));
        }
        if status != 200 {
            prop_assert_eq!(Post::response(status, body), Err(UnexpectedStatus(status)));
        }
    }
}

#[test]
fn get_has_no_body() {
    assert_eq!(Get::METHOD, "GET");
    let get = Get(ShortId([1; 8]));
    assert_eq!(get.body(), &[]);
    assert_eq!(get.body().len(), 0);
}

#[test]
fn get_distinguishes_payload_from_timeout() {
    assert_eq!(
        Get::response(200, b"payload".to_vec()),
        Ok(Some(b"payload".to_vec()))
    );
    assert_eq!(Get::response(200, Vec::new()), Ok(Some(Vec::new())));
    assert_eq!(Get::response(202, Vec::new()), Ok(None));
}

#[test]
fn post_sends_the_payload() {
    let post = Post {
        id: ShortId([2; 8]),
        payload: b"payload".to_vec(),
    };
    assert_eq!(Post::METHOD, "POST");
    assert_eq!(post.body(), b"payload");
    assert_eq!(Post::response(200, Vec::new()), Ok(()));
}
