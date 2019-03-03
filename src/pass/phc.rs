use base64;
use nom::types::CompleteByteSlice;
use std::collections::HashMap;
use nom::{call, complete, cond, do_parse, named, opt, tag, take_while, fold_many0, take_while1};

fn from_b64(data: Option<Vec<u8>>) -> Option<Vec<u8>> {
    match data {
        Some(v) => match v.len() {
            0 => None,
            _ => match base64::decode_config(v.as_slice(), base64::STANDARD_NO_PAD) {
                Ok(r) => Some(r),
                Err(_) => None,
            },
        },
        None => None,
    }
}

fn to_b64(data: &[u8]) -> String {
    base64::encode_config(data, base64::STANDARD_NO_PAD)
}

#[inline]
fn is_b64(chr: u8) -> bool {
    (chr >= 0x41 && chr <= 0x5a) || // A-Z
    (chr >= 0x61 && chr <= 0x7a) || // a-z
    (chr >= 0x30 && chr <= 0x39) || // 0-9
    chr == 0x2b || // +
    chr == 0x2f // /
}

fn is_id_char(chr: u8) -> bool {
    (chr >= 0x61 && chr <= 0x7a) || // a-z
    (chr >= 0x30 && chr <= 0x39) || // 0-9
    chr == 0x2d // -
}

fn is_param_name_char(chr: u8) -> bool {
    (chr >= 0x61 && chr <= 0x7a) || // a-z
    (chr >= 0x30 && chr <= 0x39) || // 0-9
    chr == 0x2d // -
}

fn is_param_value_char(chr: u8) -> bool {
    (chr >= 0x41 && chr <= 0x5a) || // A-Z
    (chr >= 0x61 && chr <= 0x7a) || // a-z
    (chr >= 0x30 && chr <= 0x39) || // 0-9
    chr == 0x2b || // +
    chr == 0x2d || // -
    chr == 0x2e || // .
    chr == 0x2f // /
}

named!(
    get_id<CompleteByteSlice, String>,
    do_parse!(
        tag!("$") >>
        id: take_while1!(is_id_char) >>
        (String::from_utf8(id.to_vec()).unwrap())
    )
);

named!(
    get_phc_part<CompleteByteSlice, Vec<u8>>,
    do_parse!(
        tag!("$") >>
        data: take_while!(is_b64) >>
        (data.to_vec())
    )
);

named!(
    get_param_elem<CompleteByteSlice, (String, String)>,
    do_parse!(
        name: take_while1!(is_param_name_char) >>
        tag!("=") >>
        value: take_while1!(is_param_value_char) >>
        opt!(complete!(tag!(","))) >>
        (String::from_utf8(name.to_vec()).unwrap(), String::from_utf8(value.to_vec()).unwrap())
    )
);

named!(
    get_params<CompleteByteSlice, HashMap<String, String>>,
    fold_many0!(
        get_param_elem,
        HashMap::new(),
        |mut hm: HashMap<_, _>, (k, v)| {
            hm.insert(k, v);
            hm
        }
    )
);

named!(
    parse_params<CompleteByteSlice, HashMap<String, String>>,
    do_parse!(
        tag!("$") >>
        params: get_params >>
        (params)
    )
);

named!(
    get_phc<CompleteByteSlice, PHCData>,
    do_parse!(
        id: get_id >>
        parameters: opt!(parse_params) >>
        salt: cond!(parameters.is_some(), get_phc_part) >>
        hash: cond!(salt.is_some(), get_phc_part) >>
        (PHCData {
            id: id,
            parameters: match parameters {
                Some(p) => p,
                None => HashMap::new(),
            },
            salt: from_b64(salt),
            hash: from_b64(hash),
        })
    )
);

pub struct PHCData {
    pub id: String,
    pub parameters: HashMap<String, String>,
    pub salt: Option<Vec<u8>>,
    pub hash: Option<Vec<u8>>,
}

impl PHCData {
    pub fn from_bytes(s: &[u8]) -> Result<PHCData, ()> {
        match get_phc(CompleteByteSlice(s)) {
            Ok((r, v)) => match r.len() {
                0 => Ok(v),
                _ => Err(()),
            },
            Err(_) => Err(()),
        }
    }

    pub fn to_string(&self) -> Result<String, ()> {
        if self.id.is_empty() {
            return Err(());
        }
        let mut res = String::from("$");
        res += self.id.as_str();

        if self.parameters.is_empty() && self.salt.is_none() {
            return Ok(res);
        }
        res += "$";
        for (i, (k, v)) in self.parameters.iter().enumerate() {
            res += &match i {
                0 => format!("{}={}", k, v),
                _ => format!(",{}={}", k, v),
            };
        }

        match self.salt {
            Some(ref s) => {
                res += "$";
                res += to_b64(s).as_str();
                match self.hash {
                    Some(ref h) => {
                        res += "$";
                        res += to_b64(h).as_str();
                        Ok(res)
                    }
                    None => Ok(res),
                }
            }
            None => Ok(res),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PHCData;

    #[test]
    fn test_to_string_same() {
        let data = [
            "$test",
            "$test$i=42",
            "$test$$YXN1cmUu",
            "$test$i=42$YXN1cmUu",
            "$test$i=42$YXN1cmUu$YW55IGNhcm5hbCBwbGVhc3Vy",
            "$test$$YXN1cmUu$YW55IGNhcm5hbCBwbGVhc3Vy",
            "$pbkdf2$i=1000$RSF4Aw$xvdfA4H7QJQ1w/4jGcjBEIjCvsc",
            "$pbkdf2-sha256$t-y=./42+a-1$RSF4Aw$xvdfA4H7QJQ1w/4jGcjBEIjCvsc",
            "$pbkdf2$$RSF4Aw",
            "$pbkdf2$i=21000$RSF4Aw$LwCbGeQoBZIraYoDZ8Oe/PxdJHc",
        ];
        for d in data.iter() {
            match PHCData::from_bytes(&d.to_string().into_bytes()) {
                Ok(r) => match r.to_string() {
                    Ok(s) => {
                        assert_eq!(s, d.to_string());
                    }
                    Err(_) => assert!(false),
                },
                Err(_) => assert!(false),
            }
        }
    }

    #[test]
    fn test_to_string_diff() {
        let data = [
            ("$test$", "$test"),
            ("$test$$", "$test"),
            ("$test$$YXN1cmUu$", "$test$$YXN1cmUu"),
            ("$test$i=42$YXN1cmUu$", "$test$i=42$YXN1cmUu"),
        ];
        for &(d, c) in data.iter() {
            match PHCData::from_bytes(&d.to_string().into_bytes()) {
                Ok(r) => match r.to_string() {
                    Ok(s) => {
                        assert_eq!(s, c);
                    }
                    Err(_) => assert!(false),
                },
                Err(_) => assert!(false),
            }
        }
    }

    #[test]
    fn test_valid_data_id() {
        match PHCData::from_bytes(&"$dummy".to_string().into_bytes()) {
            Ok(phc) => {
                assert_eq!(phc.id, "dummy".to_string());
                assert!(phc.parameters.is_empty());
                assert_eq!(phc.salt, None);
                assert_eq!(phc.hash, None);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_valid_data_params() {
        match PHCData::from_bytes(&"$dummy$i=42".to_string().into_bytes()) {
            Ok(phc) => {
                assert_eq!(phc.id, "dummy".to_string());
                assert_eq!(phc.parameters.len(), 1);
                match phc.parameters.get("i") {
                    Some(v) => assert_eq!(v, "42"),
                    None => assert!(false),
                }
                assert_eq!(phc.salt, None);
                assert_eq!(phc.hash, None);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_valid_data_salt() {
        match PHCData::from_bytes(&"$dummy$i=42$YXN1cmUu".to_string().into_bytes()) {
            Ok(phc) => {
                assert_eq!(phc.id, "dummy".to_string());
                assert_eq!(phc.parameters.len(), 1);
                match phc.parameters.get("i") {
                    Some(v) => assert_eq!(v, "42"),
                    None => assert!(false),
                }
                match phc.salt {
                    Some(p) => assert_eq!(p, vec![0x61, 0x73, 0x75, 0x72, 0x65, 0x2e]),
                    None => assert!(false),
                };
                assert_eq!(phc.hash, None);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_valid_data_full() {
        match PHCData::from_bytes(&"$dummy$i=42$YXN1cmUu$YW55IGNhcm5hbCBwbGVhc3Vy"
            .to_string()
            .into_bytes())
        {
            Ok(phc) => {
                assert_eq!(phc.id, "dummy".to_string());
                assert_eq!(phc.parameters.len(), 1);
                match phc.parameters.get("i") {
                    Some(v) => assert_eq!(v, "42"),
                    None => assert!(false),
                }
                match phc.salt {
                    Some(p) => assert_eq!(p, vec![0x61, 0x73, 0x75, 0x72, 0x65, 0x2e]),
                    None => assert!(false),
                };
                match phc.hash {
                    Some(p) => assert_eq!(
                        p,
                        vec![
                            0x61, 0x6e, 0x79, 0x20, 0x63, 0x61, 0x72, 0x6e, 0x61, 0x6c, 0x20, 0x70,
                            0x6c, 0x65, 0x61, 0x73, 0x75, 0x72,
                        ]
                    ),
                    None => assert!(false),
                };
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_multiple_params() {
        match PHCData::from_bytes(&"$dummy$i=42,plop=asdfg,21=abcd12efg$YXN1cmUu"
            .to_string()
            .into_bytes())
        {
            Ok(phc) => {
                assert_eq!(phc.parameters.len(), 3);
                match phc.parameters.get("i") {
                    Some(v) => assert_eq!(v, "42"),
                    None => assert!(false),
                }
                match phc.parameters.get("plop") {
                    Some(v) => assert_eq!(v, "asdfg"),
                    None => assert!(false),
                }
                match phc.parameters.get("21") {
                    Some(v) => assert_eq!(v, "abcd12efg"),
                    None => assert!(false),
                }
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn test_invalid_data() {
        let data = [
            "",                                               // does not start with $<id>
            "$",                                              // still no id
            "$@zerty",                                        // id must be alphanumerical
            "$test$YXN1cmUu",                                 // parameters may not be ommited
            "$test$=42",                                      // missing parameter name
            "$test$i@=42",           // parameter name must be alphanumerical
            "$test$i=?",             // parameter value must be alphanumerical
            "$test$i",               // missing parameter value and delimiter
            "$test$i=",              // missing parameter value
            "$test$i=$YXN1cmUu",     // missing parameter value
            "$test$i=42$YXN1cmUr%w", // invalid character in salt
            "$test$i=42$YXN1cmUr%w$YW55IGNhcm5hbCBwbGVhc3Vy", // invalid character in salt
            "$test$i=$YXN1cmUu$YW55IGNhcm5hbCBwbGVhc3V=", // no padding allowed
        ];
        for s in data.iter() {
            match PHCData::from_bytes(&s.to_string().into_bytes()) {
                Ok(_) => assert!(false),
                Err(_) => assert!(true),
            }
        }
    }
}
