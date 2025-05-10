pub const OWNER_READ: u64 = 1 << 0;
pub const OWNER_WRITE: u64 = 1 << 1;
pub const OWNER_EXECUTE: u64 = 1 << 2;
pub const GROUP_READ: u64 = 1 << 3;
pub const GROUP_WRITE: u64 = 1 << 4;
pub const GROUP_EXECUTE: u64 = 1 << 5;
pub const OTHER_READ: u64 = 1 << 6;
pub const OTHER_WRITE: u64 = 1 << 7;
pub const OTHER_EXECUTE: u64 = 1 << 8;

pub const EXTENDED_PERMISSIONS: u64 = 1 << 63;

pub enum PermissionLevel {
    Owner,
    Group,
    Other,
}

impl PermissionLevel {
    pub const fn get_standard_shift(&self) -> u64 {
        match self {
            PermissionLevel::Owner => 0,
            PermissionLevel::Group => 3,
            PermissionLevel::Other => 6,
        }
    }
}

pub enum PermissionType {
    Read,
    Write,
    Execute,
}

impl PermissionType {
    pub const fn get_standard_value(&self) -> u64 {
        match self {
            PermissionType::Read => OWNER_READ,
            PermissionType::Write => OWNER_WRITE,
            PermissionType::Execute => OWNER_EXECUTE,
        }
    }
}

pub struct Permissions(pub u64);

impl Permissions {
    pub const fn to_u64(&self) -> u64 {
        self.0
    }

    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    pub const fn can(&self, level: PermissionLevel, permission: PermissionType) -> bool {
        (self.0 & (permission.get_standard_value() << level.get_standard_shift())) != 0
    }

    pub const fn set(&mut self, level: PermissionLevel, permission: PermissionType) {
        self.0 |= permission.get_standard_value() << level.get_standard_shift();
    }
}

#[macro_export]
macro_rules! permissions {
    ($($level:ident : $permission:ident),*) => {{
        let mut permissions = $crate::data::permissions::Permissions(0);
        $(
            permissions.set($crate::data::permissions::PermissionLevel::$level, $crate::data::permissions::PermissionType::$permission);
        )*
        permissions
    }};
}
