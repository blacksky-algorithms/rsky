#!/usr/bin/env python3
import re
import glob

# Pattern to match the local parse_timestamp function (with varying comment styles)
PARSE_TIMESTAMP_PATTERN = r'''    /// Parse .*? timestamp.*?
    fn parse_timestamp\(timestamp: &str\) -> Result<DateTime<Utc>, IndexerError> \{
        DateTime::parse_from_rfc3339\(timestamp\)
            \.map\(\|dt\| dt\.with_timezone\(&Utc\)\)
            \.map_err\(\|e\| \{
                IndexerError::Serialization\(format!\("Invalid timestamp '\{\}': \{\}", timestamp, e\)\)
            \}\)
    \}'''

def fix_plugin(file_path):
    with open(file_path, 'r') as f:
        content = f.read()

    # Remove the local parse_timestamp function
    updated = re.sub(PARSE_TIMESTAMP_PATTERN, '', content, flags=re.MULTILINE | re.DOTALL)

    # Replace Self::parse_timestamp with super::parse_timestamp
    updated = updated.replace('Self::parse_timestamp(', 'super::parse_timestamp(')

    with open(file_path, 'w') as f:
        f.write(updated)

    print(f"Fixed {file_path}")

# Fix all plugin files
for plugin_file in glob.glob('/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/*.rs'):
    if 'mod.rs' not in plugin_file:  # Skip mod.rs
        try:
            fix_plugin(plugin_file)
        except Exception as e:
            print(f"Error fixing {plugin_file}: {e}")

print("All plugins updated!")
