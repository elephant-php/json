#![cfg_attr(windows, feature(abi_vectorcall))]
use ext_php_rs::prelude::*;
use ext_php_rs::types::{Zval, ZendHashTable};
use serde_json::{Value, Map};

#[php_class]
#[derive(Default)]
pub struct Json;

#[php_impl]
impl Json {
    pub fn decode(json: String, as_array: Option<bool>, depth: Option<i64>) -> PhpResult<Zval> {
        let config = DecodeConfig {
            as_array: as_array.unwrap_or(false),
            max_depth: depth.unwrap_or(512),
        };

        JsonDecoder::new(config).decode(&json)
    }

    pub fn encode(value: &mut Zval, options: Option<i64>) -> Result<String, String> {
        let config = EncodeConfig::from_flags(options.unwrap_or(0));
        JsonEncoder::new(config).encode(value)
    }

    pub fn validate(json: String) -> bool {
        serde_json::from_str::<Value>(&json).is_ok()
    }
}

#[php_function]
pub fn json_decode(json: String, as_array: Option<bool>, depth: Option<i64>) -> PhpResult<Zval> {
    Json::decode(json, as_array, depth)
}

#[php_function]
pub fn json_encode(value: &mut Zval, options: Option<i64>) -> Result<String, String> {
    Json::encode(value, options)
}

#[php_function]
pub fn json_validate(json: String) -> bool {
    Json::validate(json)
}

struct DecodeConfig {
    as_array: bool,
    max_depth: i64,
}

struct JsonDecoder {
    config: DecodeConfig,
}

impl JsonDecoder {
    fn new(config: DecodeConfig) -> Self {
        Self { config }
    }

    fn decode(&self, json: &str) -> PhpResult<Zval> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| format!("JSON syntax error: {}", e))?;

        self.convert(value, 0)
    }

    fn convert(&self, value: Value, depth: i64) -> PhpResult<Zval> {
        if depth > self.config.max_depth {
            return Err("Maximum nesting depth exceeded".into());
        }

        match value {
            Value::Null => Ok(self.make_null()),
            Value::Bool(b) => {
                let mut zval = Zval::new();
                zval.set_bool(b);
                Ok(zval)
            }
            Value::Number(n) => Ok(self.convert_number(n)),
            Value::String(s) => Ok(self.make_string(&s)),
            Value::Array(arr) => self.convert_array(arr, depth),
            Value::Object(obj) => self.convert_object(obj, depth),
        }
    }

    fn make_null(&self) -> Zval {
        let mut zval = Zval::new();
        zval.set_null();
        zval
    }

    fn make_string(&self, s: &str) -> Zval {
        let mut zval = Zval::new();
        zval.set_string(s, false);
        zval
    }

    fn convert_number(&self, n: serde_json::Number) -> Zval {
        if let Some(i) = n.as_i64() {
            let mut zval = Zval::new();
            zval.set_long(i);
            zval
        } else if let Some(f) = n.as_f64() {
            Zval::from(f)
        } else {
            let s = n.to_string();
            let mut zval = Zval::new();
            zval.set_string(&s, false);
            zval
        }
    }

    fn convert_array(&self, arr: Vec<Value>, depth: i64) -> PhpResult<Zval> {
        let mut result = ZendHashTable::new();

        for (i, item) in arr.into_iter().enumerate() {
            let php_val = self.convert(item, depth + 1)?;
            result.insert_at_index(i as i64, php_val)?;
        }

        let mut zval = Zval::new();
        zval.set_hashtable(result);
        Ok(zval)
    }

    fn convert_object(&self, obj: Map<String, Value>, depth: i64) -> PhpResult<Zval> {
        let mut result = ZendHashTable::new();

        for (key, val) in obj {
            let php_val = self.convert(val, depth + 1)?;
            result.insert(&*key, php_val)?;
        }

        let mut zval = Zval::new();
        zval.set_hashtable(result);
        Ok(zval)
    }
}

struct EncodeConfig {
    pretty: bool,
    unescaped_unicode: bool,
}

impl EncodeConfig {
    fn from_flags(flags: i64) -> Self {
        Self {
            pretty: (flags & 128) != 0,
            unescaped_unicode: (flags & 256) != 0,
        }
    }
}

struct JsonEncoder {
    config: EncodeConfig,
}

impl JsonEncoder {
    fn new(config: EncodeConfig) -> Self {
        Self { config }
    }

    fn encode(&self, value: &mut Zval) -> Result<String, String> {
        let json_value = self.convert(value)?;
        self.serialize(json_value)
    }

    fn convert(&self, value: &mut Zval) -> Result<Value, String> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        if value.is_true() {
            return Ok(Value::Bool(true));
        }
        if value.is_false() {
            return Ok(Value::Bool(false));
        }
        if value.is_long() {
            return self.convert_long(value);
        }
        if value.is_double() {
            return self.convert_double(value);
        }
        if value.is_string() {
            return self.convert_string(value);
        }
        if value.is_array() {
            return self.convert_array(value);
        }
        if value.is_object() {
            return self.convert_object(value);
        }

        Err("Unsupported PHP type".to_string())
    }

    fn convert_long(&self, value: &mut Zval) -> Result<Value, String> {
        value.long()
            .map(Value::from)
            .ok_or_else(|| "Failed to read integer".to_string())
    }

    fn convert_double(&self, value: &mut Zval) -> Result<Value, String> {
        value.double()
            .and_then(|f| {
                if f.is_finite() {
                    serde_json::Number::from_f64(f).map(Value::Number)
                } else {
                    Some(Value::Null)
                }
            })
            .ok_or_else(|| "Failed to read float".to_string())
    }

    fn convert_string(&self, value: &mut Zval) -> Result<Value, String> {
        value.str()
            .map(|s| Value::String(s.to_string()))
            .ok_or_else(|| "Failed to read string".to_string())
    }

    fn convert_array(&self, value: &mut Zval) -> Result<Value, String> {
        let arr = value.array()
            .ok_or_else(|| "Failed to read array".to_string())?;

        if self.is_sequential_array(&arr) {
            self.array_to_json_array(&arr)
        } else {
            self.array_to_json_object(&arr)
        }
    }

    fn convert_object(&self, value: &mut Zval) -> Result<Value, String> {
        let arr = value.array()
            .ok_or_else(|| "Failed to read object properties".to_string())?;

        self.array_to_json_object(&arr)
    }

    fn is_sequential_array(&self, arr: &ZendHashTable) -> bool {
        let mut expected_index = 0i64;

        for (key, _) in arr.iter() {
            if !key.is_long() {
                return false;
            }

            let key_str = key.to_string();
            if let Ok(index) = key_str.parse::<i64>() {
                if index != expected_index {
                    return false;
                }
                expected_index += 1;
            } else {
                return false;
            }
        }

        true
    }

    fn array_to_json_array(&self, arr: &ZendHashTable) -> Result<Value, String> {
        let mut result = Vec::new();

        for (_, val) in arr.iter() {
            let mut val_copy = val.shallow_clone();
            result.push(self.convert(&mut val_copy)?);
        }

        Ok(Value::Array(result))
    }

    fn array_to_json_object(&self, arr: &ZendHashTable) -> Result<Value, String> {
        let mut result = Map::new();

        for (key, val) in arr.iter() {
            let key_str = key.to_string();
            let mut val_copy = val.shallow_clone();
            result.insert(key_str, self.convert(&mut val_copy)?);
        }

        Ok(Value::Object(result))
    }

    fn serialize(&self, value: Value) -> Result<String, String> {
        let result = if self.config.pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        };

        result.map_err(|e| format!("JSON serialization error: {}", e))
    }
}

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
}