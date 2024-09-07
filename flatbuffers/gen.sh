flatc -r --gen-object-api data.fbs
sed -e 's/Debug, Clone, PartialEq/Serialize, Deserialize, Debug, Clone, PartialEq/g' <data_generated.rs >a.rs
sed -E 's/(extern crate flatbuffers;)/\1\nuse serde::{Serialize, Deserialize};/g' <a.rs >../src/generated/data_generated.rs
rm a.rs data_generated.rs
