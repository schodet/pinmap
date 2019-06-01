pinmap
------

pinmap is a tool to help pin mapping when designing a board with an STM32
microcontroller.

It uses a database extracted from CubeMX to construct a table of all pins and
associated signals.  This table can be open using your favorite spreadsheet to
assign functions to pins.

pinmap supports GPIO mapping using AF (alternate functions) and the old system
using REMAP.  The output table format differ to accommodate the differences.

### Extracting the database

Download CubeMX from st.com, and run the installation. In the installed
directory, there is a db directory, copy this to pinmap directory.

To reduce disk usage, pinmap uses a compressed database, run the following
command to compress it:

```
find . -exec gzip '{}' +
```
