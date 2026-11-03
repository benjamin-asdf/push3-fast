(ns push3-fast-oracle.hindsight-check
  "Re-runs the programs of a hindsight file on the oracle and compares every recorded output.

  Usage: ... hindsight-check <problem> <hindsight.edn> <max-programs>"
  (:require
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [benjamin-schwerdtner.problems.propeller.run :as run]
   [clojure.edn :as edn]
   [clojure.java.io :as io]))

(defn -main [problem file n]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [p (problems/by-name (keyword problem))
          opts (select-keys p [:output :num-outputs :input-names])
          recs (with-open [r (io/reader file)]
                 (into [] (comp (map edn/read-string) (take (parse-long (str n)))) (line-seq r)))
          checked (atom 0) bad (atom 0)]
      (doseq [{:keys [program examples]} recs
              {:keys [inputs output]} examples]
        (swap! checked inc)
        (let [got (run/execute program inputs opts)
              ;; the oracle reports no output as :no-stack-item and unset registers as :unset
              same? (if (sequential? output)
                      (and (sequential? got) (= (count got) (count output))
                           (every? true? (map (fn [a b] (if (nil? a) (= b :unset) (= a b))) output got)))
                      (= output got))]
          (when-not same?
            (swap! bad inc)
            (when (<= @bad 5) (println "MISMATCH" (pr-str program) inputs "want" (pr-str output) "got" (pr-str got))))))
      (println (format "%s: %d programs, %d examples checked, %d mismatches" problem (count recs) @checked @bad))))
  (shutdown-agents))
