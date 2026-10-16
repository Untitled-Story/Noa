//! Validate every numeric field, including nested schema objects, without an
//! intermediate JSON representation (JSON would coerce NaN/Infinity to null).
use serde::{Serialize, ser};
#[derive(Debug)]
pub(crate) struct Invalid(String);
impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Invalid {}
impl ser::Error for Invalid {
    fn custom<T: std::fmt::Display>(m: T) -> Self {
        Self(m.to_string())
    }
}
pub(crate) fn validate<T: Serialize>(v: &T) -> Result<(), String> {
    v.serialize(Check).map_err(|e| e.0)
}
struct Check;
macro_rules! primitive{($($method:ident:$ty:ty),*)=>{$(fn $method(self,_value:$ty)->Result<(),Invalid>{Ok(())})*};}
impl ser::Serializer for Check {
    type Ok = ();
    type Error = Invalid;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;
    primitive!(serialize_bool:bool,serialize_i8:i8,serialize_i16:i16,serialize_i32:i32,serialize_i64:i64,serialize_u8:u8,serialize_u16:u16,serialize_u32:u32,serialize_u64:u64,serialize_char:char,serialize_str:&str,serialize_bytes:&[u8]);
    fn serialize_f32(self, v: f32) -> Result<(), Invalid> {
        self.serialize_f64(v as f64)
    }
    fn serialize_f64(self, v: f64) -> Result<(), Invalid> {
        if v.is_finite() {
            Ok(())
        } else {
            Err(Invalid("nonfinite numeric value".into()))
        }
    }
    fn serialize_none(self) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<(), Invalid> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), Invalid> {
        Ok(())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<(), Invalid> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        v: &T,
    ) -> Result<(), Invalid> {
        v.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Invalid> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Invalid> {
        Ok(self)
    }
}
macro_rules! compound {
    ($trait:ident,$field:ident) => {
        impl ser::$trait for Check {
            type Ok = ();
            type Error = Invalid;
            fn $field<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Invalid> {
                v.serialize(Check)
            }
            fn end(self) -> Result<(), Invalid> {
                Ok(())
            }
        }
    };
}
compound!(SerializeSeq, serialize_element);
compound!(SerializeTuple, serialize_element);
compound!(SerializeTupleStruct, serialize_field);
compound!(SerializeTupleVariant, serialize_field);
impl ser::SerializeMap for Check {
    type Ok = ();
    type Error = Invalid;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Invalid> {
        v.serialize(Check)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Invalid> {
        v.serialize(Check)
    }
    fn end(self) -> Result<(), Invalid> {
        Ok(())
    }
}
macro_rules! fields {
    ($trait:ident) => {
        impl ser::$trait for Check {
            type Ok = ();
            type Error = Invalid;
            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                _: &'static str,
                v: &T,
            ) -> Result<(), Invalid> {
                v.serialize(Check)
            }
            fn end(self) -> Result<(), Invalid> {
                Ok(())
            }
        }
    };
}
fields!(SerializeStruct);
fields!(SerializeStructVariant);
