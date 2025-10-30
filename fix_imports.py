#!/usr/bin/env python3
import glob
import re

def fix_plugin(file_path):
    with open(file_path, 'r') as f:
        content = f.read()

    # Add import for parse_timestamp after the first use crate line
    if 'use crate::indexing::parse_timestamp;' not in content:
        # Find the first "use" statement and add our import after it
        lines = content.split('\n')
        insert_pos = 0
        for i, line in enumerate(lines):
            if line.startswith('use ') and 'crate::' in line:
                insert_pos = i + 1
                break

        # Insert the import
        lines.insert(insert_pos, 'use crate::indexing::parse_timestamp;')
        content = '\n'.join(lines)

    # Replace super::parse_timestamp with parse_timestamp
    content = content.replace('super::parse_timestamp(', 'parse_timestamp(')

    with open(file_path, 'w') as f:
        f.write(content)

    print(f"Fixed imports in {file_path}")

# Fix all plugin files
for plugin_file in glob.glob('/Users/rudyfraser/Projects/rsky/rsky-indexer/src/indexing/plugins/*.rs'):
    if 'mod.rs' not in plugin_file:
        try:
            fix_plugin(plugin_file)
        except Exception as e:
            print(f"Error fixing {plugin_file}: {e}")

print("All imports fixed!")
