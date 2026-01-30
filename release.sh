#!/bin/bash
set -e

BUMP=${1:-patch}

if [[ ! "$BUMP" =~ ^(patch|minor|major)$ ]]; then
  echo "Usage: $0 [patch|minor|major]"
  exit 1
fi

echo "Triggering release with $BUMP bump..."
gh workflow run release.yml -f bump="$BUMP"

echo "Waiting for workflow to start..."
sleep 2

gh run watch
