//! The 249 officially assigned ISO 3166-1 alpha-2 country codes (research R11).
//!
//! Source: Debian `iso-codes` v4.20.1, `data/iso_3166-1.json` (2026-01-01). No code has been added
//! or withdrawn since 2011 (SS); only names have changed. Adding a newly assigned code needs a
//! program upgrade.

/// Assigned codes, sorted.
pub const ASSIGNED_CODES: [[u8; 2]; 249] = [
    *b"AD", *b"AE", *b"AF", *b"AG", *b"AI", *b"AL", *b"AM", *b"AO", *b"AQ", *b"AR", *b"AS", *b"AT",
    *b"AU", *b"AW", *b"AX", *b"AZ", *b"BA", *b"BB", *b"BD", *b"BE", *b"BF", *b"BG", *b"BH", *b"BI",
    *b"BJ", *b"BL", *b"BM", *b"BN", *b"BO", *b"BQ", *b"BR", *b"BS", *b"BT", *b"BV", *b"BW", *b"BY",
    *b"BZ", *b"CA", *b"CC", *b"CD", *b"CF", *b"CG", *b"CH", *b"CI", *b"CK", *b"CL", *b"CM", *b"CN",
    *b"CO", *b"CR", *b"CU", *b"CV", *b"CW", *b"CX", *b"CY", *b"CZ", *b"DE", *b"DJ", *b"DK", *b"DM",
    *b"DO", *b"DZ", *b"EC", *b"EE", *b"EG", *b"EH", *b"ER", *b"ES", *b"ET", *b"FI", *b"FJ", *b"FK",
    *b"FM", *b"FO", *b"FR", *b"GA", *b"GB", *b"GD", *b"GE", *b"GF", *b"GG", *b"GH", *b"GI", *b"GL",
    *b"GM", *b"GN", *b"GP", *b"GQ", *b"GR", *b"GS", *b"GT", *b"GU", *b"GW", *b"GY", *b"HK", *b"HM",
    *b"HN", *b"HR", *b"HT", *b"HU", *b"ID", *b"IE", *b"IL", *b"IM", *b"IN", *b"IO", *b"IQ", *b"IR",
    *b"IS", *b"IT", *b"JE", *b"JM", *b"JO", *b"JP", *b"KE", *b"KG", *b"KH", *b"KI", *b"KM", *b"KN",
    *b"KP", *b"KR", *b"KW", *b"KY", *b"KZ", *b"LA", *b"LB", *b"LC", *b"LI", *b"LK", *b"LR", *b"LS",
    *b"LT", *b"LU", *b"LV", *b"LY", *b"MA", *b"MC", *b"MD", *b"ME", *b"MF", *b"MG", *b"MH", *b"MK",
    *b"ML", *b"MM", *b"MN", *b"MO", *b"MP", *b"MQ", *b"MR", *b"MS", *b"MT", *b"MU", *b"MV", *b"MW",
    *b"MX", *b"MY", *b"MZ", *b"NA", *b"NC", *b"NE", *b"NF", *b"NG", *b"NI", *b"NL", *b"NO", *b"NP",
    *b"NR", *b"NU", *b"NZ", *b"OM", *b"PA", *b"PE", *b"PF", *b"PG", *b"PH", *b"PK", *b"PL", *b"PM",
    *b"PN", *b"PR", *b"PS", *b"PT", *b"PW", *b"PY", *b"QA", *b"RE", *b"RO", *b"RS", *b"RU", *b"RW",
    *b"SA", *b"SB", *b"SC", *b"SD", *b"SE", *b"SG", *b"SH", *b"SI", *b"SJ", *b"SK", *b"SL", *b"SM",
    *b"SN", *b"SO", *b"SR", *b"SS", *b"ST", *b"SV", *b"SX", *b"SY", *b"SZ", *b"TC", *b"TD", *b"TF",
    *b"TG", *b"TH", *b"TJ", *b"TK", *b"TL", *b"TM", *b"TN", *b"TO", *b"TR", *b"TT", *b"TV", *b"TW",
    *b"TZ", *b"UA", *b"UG", *b"UM", *b"US", *b"UY", *b"UZ", *b"VA", *b"VC", *b"VE", *b"VG", *b"VI",
    *b"VN", *b"VU", *b"WF", *b"WS", *b"YE", *b"YT", *b"ZA", *b"ZM", *b"ZW",
];
