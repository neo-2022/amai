pub fn brand_mark_svg() -> &'static str {
    include_str!("../brand/amai_mark.svg")
}

pub fn brand_lockup_svg() -> &'static str {
    include_str!("../brand/amai_lockup.svg")
}

pub fn favicon_ico() -> &'static [u8] {
    include_bytes!("../brand/favicon.ico")
}
