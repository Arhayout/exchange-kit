use chrono::DateTime;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// RFC 3986 unreserved characters stay literal; everything else is percent-encoded
/// with uppercase hex (Huobi: `:` → `%3A`, space → `%20`).
const HUOBI_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn encode_component(value: &str) -> percent_encoding::PercentEncode<'_> {
    utf8_percent_encode(value, HUOBI_ENCODE_SET)
}

pub enum HTTPMethod {
    Options,
    Get,
    Post,
    Put,
    Delete,
    Head,
    Trace,
    Connect,
    Patch,
}

impl HTTPMethod {
    fn as_str(&self) -> &'static str {
        match self {
            HTTPMethod::Options => "OPTIONS",
            HTTPMethod::Get => "GET",
            HTTPMethod::Post => "POST",
            HTTPMethod::Put => "PUT",
            HTTPMethod::Delete => "DELETE",
            HTTPMethod::Head => "HEAD",
            HTTPMethod::Trace => "TRACE",
            HTTPMethod::Connect => "CONNECT",
            HTTPMethod::Patch => "PATCH",
        }
    }
}

pub struct Params {
    pub access_key_id: String,
    pub signature_method: String,
    pub signature_version: String,
    pub timestamp: DateTime<chrono::Utc>,
}

impl Params {
    /// Canonical query string for Huobi's pre-sign payload.
    /// Keys are already in ASCII sort order; values are RFC 3986 percent-encoded.
    pub fn format_for_signature(&self) -> String {
        let timestamp = self.timestamp.format("%Y-%m-%dT%H:%M:%S").to_string();

        format!(
            "AccessKeyId={}&SignatureMethod={}&SignatureVersion={}&Timestamp={}",
            encode_component(&self.access_key_id),
            encode_component(&self.signature_method),
            encode_component(&self.signature_version),
            encode_component(&timestamp),
        )
    }
}

pub fn sign_hmac_sha256_base64(secret: &[u8], msg: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(msg);
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        mac.finalize().into_bytes(),
    )
}

/// Signs a Huobi REST request for the default host (`api.huobi.pro`).
///
/// `path` must be the request path only (e.g. `/v1/order/orders/123`), not a
/// full URL — embedding the scheme/host here would invalidate the payload.
pub fn generate_signature(
    path: &str,
    method: &HTTPMethod,
    params: &Params,
    secret: &[u8],
) -> String {
    generate_signature_for_host("api.huobi.pro", path, method, params, secret)
}

/// Same as [`generate_signature`], but with an explicit host (regional or
/// alternate endpoints).
pub fn generate_signature_for_host(
    host: &str,
    path: &str,
    method: &HTTPMethod,
    params: &Params,
    secret: &[u8],
) -> String {
    // Huobi pre-sign text: METHOD\nhost\npath\ncanonical_query
    let payload = format!(
        "{}\n{}\n{}\n{}",
        method.as_str(),
        host,
        path,
        params.format_for_signature()
    );
    sign_hmac_sha256_base64(secret, payload.as_bytes())
}

#[cfg(test)]
mod tests {

    use chrono::{TimeZone, Utc};

    use crate::signatures::{generate_signature, HTTPMethod, Params};

    use super::sign_hmac_sha256_base64;

    #[test]
    fn test_format() {
        let params = Params {
            access_key_id: "2xxxxxx-99xxxxxx-84xxxxxx-7xxxx".to_owned(),
            signature_method: "HmacSHA256".to_owned(),
            signature_version: "2".to_owned(),
            timestamp: Utc.with_ymd_and_hms(2017, 5, 11, 15, 19, 30).unwrap(),
        };

        let formatted_params = params.format_for_signature();

        assert_eq!(formatted_params, "AccessKeyId=2xxxxxx-99xxxxxx-84xxxxxx-7xxxx&SignatureMethod=HmacSHA256&SignatureVersion=2&Timestamp=2017-05-11T15%3A19%3A30".to_owned())
    }

    #[test]
    fn test_signing() {
        let test_values = [
            (
                "secr1fasldkfjalsdkjf".as_bytes(),
                "m1_asdf939239841131".as_bytes(),
                "S+pBn9PZW20yWO7GCbu8Ddllb3GVneVI8Gg2vcTWi3g=".to_owned(),
            ),
            (
                "secr2c.jvxc.vvx.v,xx".as_bytes(),
                "m2_12111112233".as_bytes(),
                "zYZXdRfjlTNFxt2/10CHReXbY1fv8MLdjzdtt9reuMk=".to_owned(),
            ),
            (
                "secr3ssaa".as_bytes(),
                "m3_asldfjaslkdfj".as_bytes(),
                "DUjtxP+/FNtecpO+zcOq06inPK68SZrH4BN8/UwkOko=".to_owned(),
            ),
            (
                "secr4alsdfaaa".as_bytes(),
                "alskjdfalskdjf921020_m4".as_bytes(),
                "2Co7sVbQ7BWpcC0R34MAT9JxD4V6i7OhZzh4YIABVrw=".to_owned(),
            ),
            (
                "aslfjaslkdfjaslaaa_secr5".as_bytes(),
                "asdkfjalsdkf+m5+lksdfjalskdjf".as_bytes(),
                "fZsYbaf/hfhNc9oDLunefQUHlXSR6icf14/MA7IxCbA=".to_owned(),
            ),
        ];

        for (input_secret, input_msg, expected_output) in test_values {
            assert_eq!(
                sign_hmac_sha256_base64(input_secret, input_msg),
                expected_output
            );
        }
    }

    #[test]
    fn test_hash() {
        let params = Params {
            access_key_id: "2xxxxxx-99xxxxxx-84xxxxxx-7xxxx".to_owned(),
            signature_method: "HmacSHA256".to_owned(),
            signature_version: "2".to_owned(),
            timestamp: Utc.with_ymd_and_hms(2017, 5, 11, 15, 19, 30).unwrap(),
        };

        let order_id = "1234567890";
        let secret = "SUPER_SECRET".as_bytes();

        let signature = generate_signature(
            &format!("/v1/order/orders/{order_id}"),
            &HTTPMethod::Get,
            &params,
            secret,
        );

        assert_eq!(signature, "vmMEoVzzD049no2GMvQLkcVW0YGpQWTHIJdOTTotht0=");
    }

    #[test]
    fn test_hashes() {
        let input = [
            (
                HTTPMethod::Options,
                "/v1/order/orders".to_owned(),
                "SECRET_1".as_bytes(),
                Params {
                    access_key_id: "3xxxxxx-77xxxxxx-68xxxxxx-5xxxx".to_owned(),
                    signature_method: "HmacSHA1".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "22.1".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2008, 2, 26, 5, 5, 23).unwrap(),
                },
                "i3ihZBPu+K0QktB5KpM/ok238ct+VXcOC6d0RX9mwy8=".to_owned(),
            ),
            (
                HTTPMethod::Get,
                "/v2/order/orders".to_owned(),
                "SECRET_2".as_bytes(),
                Params {
                    access_key_id: "4xxxxxx-22xxxxxx-94xxxxxx-3xxxx".to_owned(),
                    signature_method: "HmacSHA384".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "9".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2008, 4, 23, 11, 59, 45).unwrap(),
                },
                "76wB+En0qJZ8R/lLXvFGQthFEjmifv9kHBn6AqmCmis=".to_owned(),
            ),
            (
                HTTPMethod::Post,
                "/v3/order/order".to_owned(),
                "SECRET_3".as_bytes(),
                Params {
                    access_key_id: "5xxxxxx-11xxxxxx-75xxxxxx-9xxxx".to_owned(),
                    signature_method: "HmacSHA512".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "87".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2014, 11, 4, 13, 29, 1).unwrap(),
                },
                "iebKNPwujPL4PW24gxneK/rIe/sDpyWU7YPHpyfPpWU=".to_owned(),
            ),
            (
                HTTPMethod::Put,
                "/v4/summary".to_owned(),
                "SECRET_4".as_bytes(),
                Params {
                    access_key_id: "6xxxxxx-44xxxxxx-81xxxxxx-2xxxx".to_owned(),
                    signature_method: "MD5".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "4".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2018, 6, 29, 15, 47, 59).unwrap(),
                },
                "nT+tAa9bP8X79fLNI3w1eMKxQ0FjPr/yZEHjXwd+A5U=".to_owned(),
            ),
            (
                HTTPMethod::Delete,
                "/v5/common/timestamp".to_owned(),
                "SECRET_5".as_bytes(),
                Params {
                    access_key_id: "7xxxxxx-66xxxxxx-33xxxxxx-8xxxx".to_owned(),
                    signature_method: "SHA-1".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "5.9".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2019, 2, 24, 3, 1, 4).unwrap(),
                },
                "71jj7LGjX3GptKXJ6z9o6QU/HMxAxTRCaW1MiySExO8=".to_owned(),
            ),
            (
                HTTPMethod::Head,
                "/v6/reference/currencies".to_owned(),
                "SECRET_6".as_bytes(),
                Params {
                    access_key_id: "8xxxxxx-88xxxxxx-12xxxxxx-1xxxx".to_owned(),
                    signature_method: "SHA-256".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "a14".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2019, 3, 7, 5, 9, 4).unwrap(),
                },
                "Ogg5KTA0WSVHVHLWDU/2uj/BcTiaSZhDV1uHM4uLJ50=".to_owned(),
            ),
            (
                HTTPMethod::Trace,
                "/v7/account/accounts".to_owned(),
                "SECRET_7".as_bytes(),
                Params {
                    access_key_id: "9xxxxxx-55xxxxxx-57xxxxxx-6xxxx".to_owned(),
                    signature_method: "SHA-512".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "89-beta.1".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2020, 2, 4, 23, 59, 59).unwrap(),
                },
                "wBUCejUokKNynzlos92wq+M9FWmmRfluVjsLRY2FDw0=".to_owned(),
            ),
            (
                HTTPMethod::Connect,
                "/v8/account/transfer".to_owned(),
                "SECRET_8".as_bytes(),
                Params {
                    access_key_id: "1xxxxxx-99xxxxxx-45xxxxxx-4xxxx".to_owned(),
                    signature_method: "PBKDF2WithHmacSHA1".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "gavin_belson".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2021, 4, 24, 0, 0, 1).unwrap(),
                },
                "eo14k+SG5WDLnQI9D8lIetj5ExFbDkhuFqhGDlemAZk=".to_owned(),
            ),
            (
                HTTPMethod::Patch,
                "/v9/point/transfer".to_owned(),
                "SECRET_9".as_bytes(),
                Params {
                    access_key_id: "0xxxxxx-33xxxxxx-29xxxxxx-0xxxx".to_owned(),
                    signature_method: "PBKDF2WithHmacSHA512".to_owned(), // Note, only for data variety, the actual hash should be `HmacSHA256`
                    signature_version: "signature_version".to_owned(),
                    timestamp: Utc.with_ymd_and_hms(2022, 6, 29, 2, 11, 0).unwrap(),
                },
                "T4QhN+K7Z8Vzj6ChaRdG0nkqZb+EaibqDvh1unlWAJg=".to_owned(),
            ),
        ];

        for (method, uri, secret, params, expected_signature) in input {
            let signature = generate_signature(&uri, &method, &params, secret);

            assert_eq!(signature, expected_signature);
        }
    }
}
