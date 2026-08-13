use foreign_types::{ForeignTypeRef, foreign_type};

use super::ffi::{FcConfig, FcConfigDestroy, FcConfigGetCurrent, FcConfigGetFonts};
use super::{FontSetRef, SetName};

foreign_type! {
    pub unsafe type Config {
        type CType = FcConfig;
        fn drop = FcConfigDestroy;
    }
}

impl Config {
    pub fn get_current() -> &'static ConfigRef {
        unsafe { ConfigRef::from_ptr(FcConfigGetCurrent()) }
    }
}

impl ConfigRef {
    pub fn get_fonts(&self, set: SetName) -> &FontSetRef {
        unsafe {
            let ptr = FcConfigGetFonts(self.as_ptr(), set as u32);
            FontSetRef::from_ptr(ptr)
        }
    }
}
