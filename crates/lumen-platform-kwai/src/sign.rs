//! Request signature for Kwai GLOBAL REST (`sig` parameter), per weteta/kwai-live:
//! md5( for keys sorted ascending: `key=url_decode(value)`, excluding sig/__NS_sig3/__NStokensig,
//! then the salt ), hex-encoded. The salt is tied to an old app version and may be obsolete.
use md5::{Digest, Md5};
use std::collections::BTreeMap;

pub const SALT: &str = "382700b563f4";

pub fn sign(params: &[(String, String)], salt: &str) -> String {
    // Later duplicates win, like the reference's `{...query, ...body}` merge.
    let map: BTreeMap<&str, &str> = params.iter().map(|(k, v)| (k.as_str(), v.as_str())).filter(|(k, _)| !matches!(*k, "sig" | "__NS_sig3" | "__NStokensig")).collect();
    let mut h = Md5::new();
    for (k, v) in map {
        h.update(format!("{k}={}", percent_decode(v)));
    }
    h.update(salt);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_reference_algorithm() {
        let p = |k: &str, v: &str| (k.to_string(), v.to_string());
        let params = vec![p("time_zone", "UTC%20America%2FNew_York"), p("app", "0"), p("sig", "x"), p("__NS_sig3", ""), p("app", "1")];
        // Oracle: echo -n "app=1time_zone=UTC America/New_York382700b563f4" | md5sum
        assert_eq!(sign(&params, SALT), "6a819e56b4b37a7591be262a2c7d0438");
        assert_eq!(percent_decode("a%2Fb%zz%"), "a/b%zz%");
    }
}
