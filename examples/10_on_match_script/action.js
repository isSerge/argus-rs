// Example action that modifies the context object
console.log("=== Action Triggered ===");
console.log("Monitor:", context.monitor_name);
console.log("Block Number:", context.block_number);

// Modify existing fields only (can't add new fields to strongly-typed struct)
context.monitor_name = "Enhanced " + context.monitor_name;

console.log("Modified monitor name:", context.monitor_name);
console.log("========================");
