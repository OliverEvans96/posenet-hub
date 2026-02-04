#!/usr/bin/env python3
"""
Blender script: pick one vertex per PoseNet keypoint (edit mode), then print
a table of world-space coordinates matching the Pose3D structure (posenet-hub).

Usage: In Blender, go to Edit Mode, select a mesh. Run this script (Run
button in Script Editor). The UI stays responsive: for each keypoint, select one or more vertices in the 3D view (multiple = midpoint), then press SPACE (or ENTER) to confirm.
Press ESC to cancel and print whatever was collected so far.
Output goes to the terminal where Blender was started.
"""

import bpy
import bmesh

# Same 17 keypoints as proto/geometry.proto Pose3D (order matters)
POINTS_OF_INTEREST = [
    "nose",
    "left_eye",
    "right_eye",
    "left_ear",
    "right_ear",
    "left_shoulder",
    "right_shoulder",
    "left_elbow",
    "right_elbow",
    "left_wrist",
    "right_wrist",
    "left_hip",
    "right_hip",
    "left_knee",
    "right_knee",
    "left_ankle",
    "right_ankle",
]


def get_selected_vertex_world_co(context):
    """Return world (x, y, z) of the selected vertex, or midpoint of multiple selected, or None if none."""
    obj = context.edit_object
    if obj is None:
        return None
    bm = bmesh.from_edit_mesh(obj.data)
    selected = [v for v in bm.verts if v.select]
    if not selected:
        return None
    world_coords = [obj.matrix_world @ v.co for v in selected]
    n = len(world_coords)
    x = sum(c.x for c in world_coords) / n
    y = sum(c.y for c in world_coords) / n
    z = sum(c.z for c in world_coords) / n
    return (float(x), float(y), float(z))


def print_table(collected):
    """Pretty-print keypoint name and x, y, z (to terminal)."""
    if not collected:
        print("(no points collected)")
        return
    name_w = max(len(n) for n, _ in collected) if collected else 4
    name_w = max(name_w, 4)
    fmt = f"  {{name:<{name_w}}}  {{x:>12.6f}}  {{y:>12.6f}}  {{z:>12.6f}}"
    header = f"  {'name':<{name_w}}  {'x':>12}  {'y':>12}  {'z':>12}"
    print(header)
    print("  " + "-" * (name_w + 3 * 13 + 6))
    for name, (x, y, z) in collected:
        print(fmt.format(name=name, x=x, y=y, z=z))


def parse_previous_output(text):
    """
    Parse table output (from print_table) back into a list of (name, (x, y, z))
    in POINTS_OF_INTEREST order. Used to resume from a previous run.
    """
    name_to_co = {}
    for line in text.strip().splitlines():
        tokens = line.split()
        if len(tokens) >= 4 and tokens[0] in POINTS_OF_INTEREST:
            try:
                x, y, z = float(tokens[1]), float(tokens[2]), float(tokens[3])
                name_to_co[tokens[0]] = (x, y, z)
            except ValueError:
                pass
    return [(name, name_to_co[name]) for name in POINTS_OF_INTEREST if name in name_to_co]


class POSE_OT_vertex_picker(bpy.types.Operator):
    """Pick one vertex per pose keypoint (SPACE to confirm, ESC to cancel)."""
    bl_idname = "pose.vertex_picker"
    bl_label = "Pick vertices for pose keypoints"
    bl_options = {"REGISTER"}

    def modal(self, context, event):
        if event.type in ("SPACE", "RET", "NUMPAD_ENTER") and event.value == "PRESS":
            co = get_selected_vertex_world_co(context)
            name = POINTS_OF_INTEREST[self._index]
            if co is not None:
                self._collected.append((name, co))
                self._index += 1
                if self._index >= len(POINTS_OF_INTEREST):
                    print("\n--- All points (world space) ---")
                    print_table(self._collected)
                    self.report({"INFO"}, "Done. See terminal for table.")
                    return {"FINISHED"}
                next_name = POINTS_OF_INTEREST[self._index]
                self.report(
                    {"INFO"},
                    f"({self._index}/{len(POINTS_OF_INTEREST)}) Select vertex for '{next_name}', then SPACE",
                )
            else:
                self.report({"WARNING"}, "Select at least one vertex, then press SPACE")
        elif event.type == "ESC":
            print("\n--- Cancelled; points collected so far ---")
            print_table(self._collected)
            self.report({"INFO"}, "Cancelled. See terminal for table.")
            return {"CANCELLED"}
        return {"PASS_THROUGH"}

    def invoke(self, context, event):
        if context.mode != "EDIT_MESH":
            self.report({"ERROR"}, "Switch to Edit Mode first (Tab on the mesh)")
            return {"CANCELLED"}
        self._collected = parse_previous_output(PREVIOUS_OUTPUT)
        self._index = len(self._collected)
        if self._index >= len(POINTS_OF_INTEREST):
            print("\n--- All points (from PREVIOUS_OUTPUT + world space) ---")
            print_table(self._collected)
            self.report({"INFO"}, "Already complete. See terminal for table.")
            return {"FINISHED"}
        context.window_manager.modal_handler_add(self)
        next_name = POINTS_OF_INTEREST[self._index]
        self.report(
            {"INFO"},
            f"({self._index + 1}/{len(POINTS_OF_INTEREST)}) Resuming: select vertex for '{next_name}', then SPACE. ESC to cancel.",
        )
        return {"RUNNING_MODAL"}


def register():
    bpy.utils.register_class(POSE_OT_vertex_picker)


def unregister():
    bpy.utils.unregister_class(POSE_OT_vertex_picker)


# Paste the last table output here to resume; leave empty to start from scratch.
PREVIOUS_OUTPUT = """
"""
# Example (paste your terminal output between the triple quotes):
# PREVIOUS_OUTPUT = """
#   name               x            y            z
#   ----  -------------- -------------- --------------
#   nose        0.123456        0.234567        0.345678
#   left_eye    0.111111        0.222222        0.333333
# """

if __name__ == "__main__":
    try:
        unregister()
    except Exception:
        pass
    register()
    bpy.ops.pose.vertex_picker("INVOKE_DEFAULT")
