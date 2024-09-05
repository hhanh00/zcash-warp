flatc -r --gen-object-api data.fbs
sed -e 's/Debug, Clone, PartialEq/Serialize, Debug, Clone, PartialEq/g' <data_generated.rs >a.rs
mv a.rs data_generated.rs
