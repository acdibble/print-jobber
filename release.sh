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
sleep 3

RUN_ID=$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')
echo "Watching run $RUN_ID..."
gh run watch "$RUN_ID"
