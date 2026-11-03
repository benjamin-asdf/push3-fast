(ns push3-fast-oracle.hindsight-check
  "Re-runs the programs of a hindsight file on the oracle and compares every recorded output.

  Programs using sin, cos or tan are counted apart: java.lang.Math differs from libm in the last
  digit for a small share of the calls.

  Usage: ... hindsight-check <problem> <spec.edn> <hindsight.edn> <max-programs>"
  (:require
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [benjamin-schwerdtner.problems.propeller.run :as run]
   [clojure.edn :as edn]
   [clojure.java.io :as io]))

(defn -main [problem spec-file file n]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [p (problems/by-name (keyword problem))
          step-limit (:max (edn/read-string (slurp spec-file)))
          opts (assoc (select-keys p [:output :num-outputs :input-names]) :step-limit step-limit)
          recs (with-open [r (io/reader file)]
                 (into [] (comp (map edn/read-string) (take (parse-long (str n)))) (line-seq r)))
          checked (atom 0) bad (atom 0) trig (atom 0)
          trig? (fn [program] (some #(re-find % (pr-str program)) [#":float_sin" #":float_cos" #":float_tan"]))]
      (doseq [{:keys [program examples]} recs
              {:keys [inputs output]} examples]
        (swap! checked inc)
        (let [got (run/execute program inputs opts)
              ;; the oracle reports no output as :no-stack-item and unset registers as :unset
              same? (if (sequential? output)
                      (and (sequential? got) (= (count got) (count output))
                           (every? true? (map (fn [a b] (if (nil? a) (= b :unset) (= a b))) output got)))
                      (= output got))]
          (cond
            same? nil
            (trig? program) (swap! trig inc)
            :else
            (do (swap! bad inc)
            (when (<= @bad 5) (println "MISMATCH" (pr-str program) inputs "want" (pr-str output) "got" (pr-str got)))))))
      (println (format "%s: %d programs, %d examples checked, %d mismatches, %d differences in sin/cos/tan programs" problem (count recs) @checked @bad @trig))))
  (shutdown-agents))
