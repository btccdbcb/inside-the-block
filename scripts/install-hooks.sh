#!/bin/sh
# Point git at the repo's hooks (pre-commit key guard).
git config core.hooksPath scripts/hooks
echo "hooks installed: $(git config core.hooksPath)"
