### classroom50 autograde: 0/6

| Test | Result | Score |
|---|---|---|
| builds a pyramid of 3 | FAIL | 0 / 3 |
| rejects a negative height | FAIL | 0 / 2 |
| prints its steps | FAIL | 0 / 1 |

<details><summary>Failure details</summary>

**builds a pyramid of 3**

```
exit 0; comparison=exact
--- input ---
3

--- expected
+++ actual stdout
@@ -1,3 +1,3 @@
-#
 ##
 ###
+####
```

**rejects a negative height**

```
timed out after 1s
--- input ---
-3

--- actual stdout ---
Height: Height: 
--- stderr ---
height must be positive
```

**prints its steps**

```
timed out after 1s
--- run command ---
python3 compile.py
--- output ---
compiling
warn
```

</details>

Status: classroom50 autograde: 0/6 (0/3 tests passed)
