#!/bin/bash
set -e

BUMP=${1:-patch}

if [[ ! "$BUMP" =~ ^(patch|minor|major)$ ]]; then
  echo "Usage: $0 [patch|minor|major]"
  exit 1
fi

# Calculate new version
LATEST=$(git tag -l 'v*' | sort -V | tail -n1 || echo "v0.0.0")
LATEST_NUM=${LATEST#v}

IFS='.' read -r MAJOR MINOR PATCH <<< "$LATEST_NUM"

case "$BUMP" in
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  patch) PATCH=$((PATCH + 1)) ;;
esac

NEW_VERSION="v${MAJOR}.${MINOR}.${PATCH}"
echo "Bumping $BUMP: $LATEST -> $NEW_VERSION"

# Check for workflow changes since last tag
if git diff --name-only "$LATEST"..HEAD | grep -q '^\.github/workflows/'; then
  echo "Workflow changes detected - creating tag locally..."
  git tag "$NEW_VERSION"
  git tag -f latest
  git push origin "$NEW_VERSION"
  git push origin latest --force
  echo "Tags pushed. Triggering build..."
  gh workflow run release.yml -f bump="$BUMP"
  sleep 3
  RUN_ID=$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')
  echo "Watching run $RUN_ID..."
  gh run watch "$RUN_ID"
else
  echo "No workflow changes - using GitHub Actions..."
  gh workflow run release.yml -f bump="$BUMP"
  sleep 3
  RUN_ID=$(gh run list --workflow=release.yml --limit=1 --json databaseId --jq '.[0].databaseId')
  echo "Watching run $RUN_ID..."
  gh run watch "$RUN_ID"
fi
