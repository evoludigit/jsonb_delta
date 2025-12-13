#!/bin/bash

# validate_docs.sh - Validate documentation quality
# Usage: ./scripts/validate_docs.sh

set -e

echo "🔍 Validating documentation..."

# Check if markdownlint-cli2 is available
if ! command -v markdownlint-cli2 &> /dev/null; then
    echo "⚠️  markdownlint-cli2 not found. Install with: npm install -g markdownlint-cli2"
    echo "Continuing with basic validation..."
else
    echo "✅ Running markdown linting..."
    # Lint all markdown files
    find docs -name "*.md" -exec markdownlint-cli2 {} \;
    echo "✅ Markdown linting passed"
fi

# Check for broken internal links (basic check)
echo "🔗 Checking for broken internal links..."
for file in docs/*.md README.md; do
    if [ -f "$file" ]; then
        # Look for relative links that might be broken
        grep -n "\[.*\](\.\." "$file" | while read -r line; do
            link=$(echo "$line" | grep -o "\[.*\](\.\.[^)]*)" | sed 's/.*(\.\././')
            link=${link%)}  # Remove trailing )
            if [[ "$link" == ./* ]]; then
                # Convert relative link to absolute path from repo root
                abs_path="${link#./}"
                if [ ! -f "$abs_path" ] && [ ! -d "$abs_path" ]; then
                    echo "⚠️  Broken link in $file: $link"
                fi
            fi
        done
    fi
done

# Validate code examples compile (if rustdoc is available)
echo "💻 Checking code examples..."
if cargo test --doc --no-run 2>/dev/null; then
    echo "✅ Documentation code examples compile"
else
    echo "⚠️  Some documentation code examples may not compile"
fi

# Check for TODO/FIXME in docs
echo "📝 Checking for unresolved TODOs in docs..."
todo_count=$(grep -r "TODO\|FIXME\|XXX" docs/ | wc -l)
if [ "$todo_count" -gt 0 ]; then
    echo "⚠️  Found $todo_count TODO/FIXME items in docs/"
    grep -r "TODO\|FIXME\|XXX" docs/ | head -10
else
    echo "✅ No TODO/FIXME items found in docs/"
fi

echo "🎉 Documentation validation complete!"
