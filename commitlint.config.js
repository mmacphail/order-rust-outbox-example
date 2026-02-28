/** @type {import('@commitlint/types').UserConfig} */
module.exports = {
  extends: ['@commitlint/config-conventional'],
  // Ignore bot-generated planning commits created by the copilot-swe-agent
  // before the conventional-commits convention was established in the repo.
  ignores: [(message) => /^Initial plan\b/.test(message.trim())],
};
