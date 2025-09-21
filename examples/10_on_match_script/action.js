// Example action that modifies the match object
console.log("=== Action Triggered ===");
console.log("Monitor:", match.monitor_name);
console.log("Block Number:", match.block_number);

// Modify existing fields only (can't add new fields to strongly-typed struct)
match.monitor_name = "Enhanced " + match.monitor_name;

console.log("Modified monitor name:", match.monitor_name);
console.log("========================");
