//! Plain-language hints for the compiler messages beginners hit most often.

/// Returns a one-sentence hint for a naga error `message`, or `None` when the message is not one
/// of the recognised beginner mistakes. `line_text` is the offending user line (may be empty).
pub fn hint_for(message: &str, line_text: &str) -> Option<String> {
    let m = message;
    let _ = line_text;
    let hint = if m.contains("automatic conversions cannot convert") && m.contains("vec4<f32>") {
        "The function returns a vec4<f32>. Wrap a single number: `return vec4<f32>(v, 0.0, 0.0, 1.0)` or `return gray(v)`."
    } else if m.contains("Abstract types may only appear in constant expressions")
        || (m.contains("Operation") && m.contains("can't work with"))
        || m.contains("automatic conversions cannot convert")
    {
        "WGSL never converts numbers implicitly: an integer and a float, or i32 and u32, cannot be mixed. Convert explicitly, e.g. `f32(pos.x)`, `i32(n)` or `u32(k)`, and give integer literals a suffix (`1u`)."
    } else if m.contains("no definition in scope for identifier") {
        "Check the spelling. Rule helpers: cell, alive, neighbours, neighbours4, laplacian, moore_sum, prev_cell, prev_alive, noise, on, off, on_if. Render helpers: gray, rgb, hsv, palette, cell_at. Post helpers: scene, prev, scene_px. Sliders are read as `params.<name>`."
    } else if m.contains("does not match the declared return type") {
        "Every path through `rule`, `shade` and `post` must end with `return vec4<f32>(...)` (or a helper such as `on_if(...)` / `gray(...)`)."
    } else if m.starts_with("expected") && m.contains("found \"=\"") {
        "Comparison is `==`; a single `=` assigns."
    } else if m.starts_with("expected `;`") {
        "The previous statement is missing its semicolon."
    } else if m.contains("invalid left-hand side of assignment") {
        "`let` values cannot change. Declare it with `var` if you assign to it later."
    } else if m.contains("Requires") && m.contains("arguments, but") {
        "Wrong number of arguments. Helpers take (x, y) in 2D and (x) for prev_cell / prev_alive; see the helper table in the README."
    } else if m.contains("Argument") && m.contains("doesn't match the type") {
        "Argument type mismatch. Coordinates for cell / alive / neighbours are i32: write `cell(i32(pos.x) - 1, i32(pos.y))`."
    } else if m.contains("Composing") && m.contains("component type") {
        "vec4<f32>(...) takes floats. Convert a bool with `f32(b)` or `select(0.0, 1.0, b)`, and an integer with `f32(n)`."
    } else {
        return None;
    };
    Some(hint.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(msg: &str, line: &str) -> String {
        hint_for(msg, line).unwrap_or_else(|| panic!("expected a hint for: {msg}"))
    }

    #[test]
    fn numeric_type_mixes_suggest_explicit_conversion() {
        let h = hint(
            "Function [5] 'rule' is invalid: Expression [2] is invalid: Abstract types may only appear in constant expressions",
            "let x: f32 = pos.x * 2.0;",
        );
        assert!(h.contains("f32("), "{h}");
        let h = hint(
            "Operation Add can't work with [2] (of type Scalar(Scalar { kind: Uint, width: 4 })) and [1] (of type Scalar(Scalar { kind: Sint, width: 4 }))",
            "",
        );
        assert!(h.contains("implicitly"), "{h}");
    }

    #[test]
    fn unknown_identifier_mentions_spelling_and_params() {
        let h = hint("no definition in scope for identifier: `nieghbours`", "");
        assert!(h.contains("spelling") && h.contains("params."), "{h}");
    }

    #[test]
    fn return_problems_point_at_vec4_return() {
        let h = hint(
            "Function [5] 'rule' is invalid: The `return` expression None does not match the declared return type Some([4])",
            "",
        );
        assert!(h.contains("return vec4<f32>"), "{h}");
        let h = hint("automatic conversions cannot convert `{AbstractFloat}` to `vec4<f32>`", "return 1.0;");
        assert!(h.contains("vec4<f32>("), "{h}");
    }

    #[test]
    fn syntax_slips_have_hints() {
        let h = hint("expected `)`, found \"=\"", "if (pos.x = 1u) {");
        assert!(h.contains("=="), "{h}");
        let h = hint("expected `;`, found \"return\"", "return vec4<f32>(a);");
        assert!(h.contains("semicolon"), "{h}");
        let h = hint("invalid left-hand side of assignment", "n = 2.0;");
        assert!(h.contains("var"), "{h}");
    }

    #[test]
    fn helper_misuse_has_hints() {
        let h = hint("Call to [1] is invalid: Requires 2 arguments, but 1 are provided", "return cell(1);");
        assert!(h.contains("arguments"), "{h}");
        let h = hint("Call to [1] is invalid: Argument 0 value [3] doesn't match the type [8]", "return cell(x, 0);");
        assert!(h.contains("i32("), "{h}");
        let h = hint("Composing 0's component type is not expected", "return vec4<f32>(a, 0.0, 0.0, 1.0);");
        assert!(h.contains("f32(") || h.contains("select("), "{h}");
    }

    #[test]
    fn unknown_messages_get_no_hint() {
        assert_eq!(hint_for("something entirely different", ""), None);
    }
}
