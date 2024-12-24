## Notes

If you want to see the version of the code I ran for these benchmark results on my machine, simply `git checkout 6623440` or `git checkout a7fc365`.

1. [a7fc365](https://github.com/afbase/rsky/tree/a7fc365) - the first version of the mst benchmark; it times out on @afbase's computer. If i had added something like `group.measurement_time(Duration::from_secs(1500));`, it likely would have been okay.
1. [6623440](https://github.com/afbase/rsky/tree/6623440) - the second version of the mst benchmark; it does not time out on @afbase's computer.  @afbase also thought of two metrics that might be useful to look at on the MST: (i) originally 
```math
d(x,y) = \sqrt{\left(h_b\left(x\right)-h_b\left(y\right)\right)^2 + e\left(x, y\right)^2 }
```
and (ii) more meaninful to tree depth 
```math
\delta(x,y) = \sqrt{\left(h_b\left(x\right)-h_b\left(y\right) \right)^2}
```
where x and y are any two possible keys in the key space and $h_b(x)$ is the number of leading zeros in the SHA-256 hash of x.  $e(x,y)$ is the edit distance between x and y.  I use $\delta$ as a metric in the second test (see output below) where it does give some idea in the max depth of the MST from the sample of keys and values, alongside some distribution of the hashes in the sample.  I think there is some correlation to MST tree depth and runtime. $h_b(x)$ is notation that comes from the [paper](https://inria.hal.science/hal-02303490/document).


### Criterion Reports

[Criterion](http://bheisler.github.io/criterion.rs/criterion/) outputs the benchmark reports in target by default.  I placed copy of the reports from my benchmark runs.  

## Output of the second test

Benchmarking mst_operations/add_records/100: Collecting 20 samples in estimated 1525.3 s (12k iterations)
```csv
sample size,mean,standard deviation,max MST depth (i.e. max zeros in a key)
100,0.58,0.88,5.00
100,0.50,0.71,3.00
100,0.34,0.69,4.00
100,0.61,0.87,4.00
100,0.53,0.73,3.00
100,0.79,1.07,6.00
100,0.42,0.72,3.00
100,0.51,0.76,3.00
100,0.53,0.80,3.00
100,0.39,0.58,2.00
100,0.59,0.76,4.00
100,0.61,0.79,3.00
100,0.58,0.76,3.00
100,0.61,0.82,4.00
100,0.70,0.91,4.00
100,0.58,0.79,3.00
100,0.55,0.77,4.00
100,0.65,0.80,3.00
100,0.45,0.64,2.00
100,0.55,0.87,4.00
```
mst_operations/add_records/100
                        time:   [129.79 ms 131.90 ms 134.32 ms]
Found 3 outliers among 20 measurements (15.00%)
  1 (5.00%) low mild
  2 (10.00%) high mild

Benchmarking mst_operations/add_records/500: Warming up for 3.0000 s500,0.49,0.68,4.00
Benchmarking mst_operations/add_records/500: Collecting 20 samples in estimated 1554.5 s (420 iterations)
```csv
sample size,mean,standard deviation,max MST depth (i.e. max zeros in a key)
500,0.57,0.79,4.00
500,0.55,0.78,5.00
500,0.54,0.77,4.00
500,0.55,0.79,5.00
500,0.49,0.74,5.00
500,0.59,0.83,4.00
500,0.60,0.82,5.00
500,0.54,0.83,6.00
500,0.49,0.67,3.00
500,0.48,0.70,4.00
500,0.48,0.74,4.00
500,0.55,0.84,5.00
500,0.47,0.73,3.00
500,0.49,0.74,4.00
500,0.61,0.85,5.00
500,0.56,0.73,4.00
500,0.50,0.71,4.00
500,0.59,0.80,4.00
500,0.54,0.77,4.00
500,0.54,0.79,5.00
```
mst_operations/add_records/500
                        time:   [6.6815 s 7.3676 s 8.0593 s]
Found 2 outliers among 20 measurements (10.00%)
  1 (5.00%) low mild
  1 (5.00%) high mild

Benchmarking mst_operations/add_records/1000: Warming up for 3.0000 s1000,0.57,0.79,5.00
Benchmarking mst_operations/add_records/1000: Collecting 20 samples in estimated 1700.1 s (40 iterations)
```csv
sample size,mean,standard deviation,max MST depth (i.e. max zeros in a key)
1000,0.52,0.76,6.00
1000,0.50,0.73,4.00
1000,0.54,0.82,5.00
1000,0.58,0.82,5.00
1000,0.54,0.78,4.00
1000,0.48,0.70,5.00
1000,0.56,0.83,7.00
1000,0.52,0.78,5.00
1000,0.52,0.77,6.00
1000,0.53,0.83,5.00
1000,0.51,0.75,4.00
1000,0.53,0.74,5.00
1000,0.56,0.78,4.00
1000,0.53,0.78,6.00
1000,0.58,0.82,5.00
1000,0.52,0.82,7.00
1000,0.54,0.76,4.00
1000,0.54,0.78,5.00
1000,0.58,0.80,4.00
1000,0.52,0.80,5.00
```
