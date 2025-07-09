#!/bin/bash

# This script converts legacy .txt pattern files from data/patterns/
# into the new .yml rule format in data/rules.converted/.

# Ensure the target directory exists
mkdir -p data/rules.converted

# Loop through each .txt file in the patterns directory
for pattern_file in data/patterns/*.txt; do
  # Get the base name of the file (e.g., "amazon" from "data/patterns/amazon.txt")
  base_name=$(basename "$pattern_file" .txt)
  
  # Define the path for the new YAML file
  output_file="data/rules.converted/${base_name}.yml"

  # Start with a clean file
  > "$output_file"

  echo "Converting ${pattern_file} to ${output_file}..."

  # Read the input file line by line
  while IFS= read -r line || [[ -n "$line" ]]; do
    # Trim leading/trailing whitespace
    trimmed_line=$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')

    # Skip empty lines and lines that are just comments
    if [[ -z "$trimmed_line" || "$trimmed_line" =~ ^# ]]; then
      continue
    fi

    # Escape single quotes within the regex to make it a valid YAML string
    escaped_regex=$(echo "$trimmed_line" | sed "s/'/''/g")

    # Append the formatted rule to the YAML file
    cat >> "$output_file" <<EOF
- name: "${base_name}"
  # This is a Stage 1 rule. It runs before any expensive DNS lookups.
  # It was converted from the legacy pattern format.
  all:
    - domain_regex: '${escaped_regex}'
EOF

  done < "$pattern_file"
done

echo "All pattern files have been converted."
