# Running the compressor tools from nodejs

The automatic compressor tool is available through Node bindings.

Currently, the Node bindings wont produce logging. To see output,
look at the response array for chunk (room) specific results.

## Requirements

NodeJS 16+

## Setting things up

1. Navigate to the correct location  

`$ cd synapse_auto_compressor`   

2. Build and install the library  
`$ yarn`

## Automatic tool example

```node
const synapseAutoCompressor = require('synapse_auto_compressor');

synapseAutoCompressor.runCompression(
    "postgresql://localhost/synapse", 
    500,
    100,
).then(results => {
    console.log(JSON.stringify(results));
});
```
