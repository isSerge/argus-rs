#!/bin/bash

# This script iterates through all numbered example directories, extracts the
# dry-run command from their README.md, and executes it, verifying that the output
# is not an empty array. If any command fails or returns an empty array, the script
# exits with an error.

export RUST_LOG=debug
set -e

EXAMPLES_DIR="examples"

# Find all numbered example directories
for example_dir in $(find "$EXAMPLES_DIR" -mindepth 1 -maxdepth 1 -type d -name "[0-9]*" | sort); do
    echo "========================================================================"
    echo "Running example: $example_dir"
    echo "========================================================================"

    readme_path="$example_dir/README.md"
    if [ ! -f "$readme_path" ]; then
        echo "WARNING: README.md not found in $example_dir. Skipping."
        continue
    fi

    # Extract the full dry-run command from the README.md
    command=$(grep -o 'cargo run --release -- dry-run .*' "$readme_path" | head -n 1)

    if [ -z "$command" ]; then
        echo "WARNING: Could not find dry-run command in $readme_path. Skipping."
        continue
    fi

    echo "Executing command: $command"
    echo "---"
    
    # Execute the command safely by parsing it into an array and running it directly.
    read -r -a cmd_array <<< "$command"
    output=$("${cmd_array[@]}")
    exit_code=$?

    if [ $exit_code -ne 0 ]; then
        echo "ERROR: Command failed with exit code $exit_code"
        echo "$output"
        exit 1
    fi

    # Trim whitespace from output for comparison
    trimmed_output=$(echo "$output" | tr -d '[:space:]')

    if [[ "$trimmed_output" == "[]" ]]; then
        echo "ERROR: Example returned an empty array."
        # Optionally print the empty array for clarity
        echo "$output"
        exit 1
    fi

    # If all checks pass, print the output
    echo "$output"
    echo
done

echo "========================================================================"
echo "All examples have been successfully verified."
echo "========================================================================"

exit 0
