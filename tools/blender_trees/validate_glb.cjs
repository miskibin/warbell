// Read-only local validation using the already-installed Khronos glTF validator.
const fs = require('node:fs');
const path = require('node:path');
const validator = require(process.env.GLTF_VALIDATOR_MODULE || 'gltf-validator');
const root = path.resolve(__dirname, '../..');
const names = ['oak_a','oak_b','birch_a','birch_b','pine_a'];

(async () => {
  const report = {};
  for (const name of names) {
    const file = path.join(root, 'assets/models/blender_trees', `${name}.glb`);
    const result = await validator.validateBytes(new Uint8Array(fs.readFileSync(file)), {uri: `${name}.glb`, maxIssues: 100});
    report[name] = result.issues;
    console.log(`${name}: ${result.issues.numErrors} errors, ${result.issues.numWarnings} warnings`);
  }
  fs.writeFileSync(path.join(root, 'art/blender_trees/gltf_validation.json'), JSON.stringify(report, null, 2));
  if (Object.values(report).some(v => v.numErrors > 0)) process.exitCode = 1;
})();
