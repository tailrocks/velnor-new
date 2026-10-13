function trim(value) {
  sub(/^[[:space:]]+/, "", value)
  sub(/[[:space:]]+$/, "", value)
  return value
}

/^\[[^]]+\][[:space:]]*$/ {
  in_tools = ($0 == "[tools]")
  next
}

in_tools {
  line = $0
  sub(/[[:space:]]+#.*/, "", line)
  separator = index(line, "=")
  if (!separator) next
  name = trim(substr(line, 1, separator - 1))
  first = substr(name, 1, 1)
  last = substr(name, length(name), 1)
  if ((first == "\"" || first == "'") && last == first) {
    name = substr(name, 2, length(name) - 2)
  }
  if (name != key) next
  count++
  literal = trim(substr(line, separator + 1))
  quote = substr(literal, 1, 1)
  if ((quote != "\"" && quote != "'") ||
      substr(literal, length(literal), 1) != quote ||
      length(literal) < 3) {
    invalid = 1
    next
  }
  value = substr(literal, 2, length(literal) - 2)
  if (value !~ /^[A-Za-z0-9._+-]+$/) invalid = 1
  result = value
}

END {
  if (count != 1 || invalid || result == "") exit 1
  print result
}
