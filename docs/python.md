# Running the compressor tools from python

Both the automatic and manual tools use PyO3 to allow the compressor
to be run from Python. 

To see any output from the tools, logging must be setup in Python before
the compressor is run.

## Setting things up

1. Create a virtual environment in the place you want to use the compressor from
(if it doesn't already exist)  
`$ virtualenv -p python3 venv`

2. Activate the virtual environment and install `maturin` (if you haven't already)  
`$ source venv/bin/activate`  
`$ pip install maturin`  

3. Navigate to the correct location  
For the automatic tool:  
`$ cd /home/synapse/rust-synapse-compress-state/auto_compressor`   
For the manual tool:  
`$ cd /home/synapse/rust-synapse-compress-state`   

3. Build and install the library  
`$ maturin develop`

This will install the relevant compressor tool into the activated virtual environment.

## Automatic tool example:

```python
import auto_compressor

auto_compressor.compress_state_events_table(
  db_url="postgresql://localhost/synapse",
  chunk_size=500,
  default_levels="100,50,25",
  number_of_chunks=100
)
```

# Manual tool example:

```python
import synapse_compress_state

synapse_compress_state.run_compression(
  db_url="postgresql://localhost/synapse",
  room_id="!some_room:example.com",
  output_file="out.sql",
  transactions=True
)
```