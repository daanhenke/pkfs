extends Node3D

## Renders a Gen 4 Pokemon field map by stitching land-data terrain chunks into
## a grid, pulling data straight from the ROM through the pkfs GDExtension. Each
## chunk is textured with the exact map texture set the game assigns it.
##
## Load a ROM by dragging a .nds file onto the window, or pass one on the command
## line: `godot --path game -- path/to/game.nds`.
##
## Controls: right mouse to look, WASD move, Q/E down/up, Shift sprint,
## mouse wheel = speed.

@export var max_cells := 500
## A field chunk is 32 tiles of 16 units = 512, regardless of overhanging
## geometry, so grid spacing is fixed rather than AABB-derived.
const CHUNK_SIZE := 512.0
@export var altitude_unit := 8.0

var _map_root: Node3D
var _label: Label
var _cam: Camera3D
var _yaw := 0.0
var _pitch := 0.0
var _move_speed := 1500.0

func _ready() -> void:
	_spawn_camera_rig()
	_setup_environment()
	_setup_hud()
	get_window().files_dropped.connect(_on_files_dropped)

	var cli := OS.get_cmdline_user_args()
	var rom := ""
	for a in cli:
		if a.to_lower().ends_with(".nds"):
			rom = a
			break
	if rom != "":
		_load_rom(rom)
	else:
		_label.text = "Drag a .nds ROM onto the window\n(or pass one: godot --path game -- game.nds)"

	if OS.get_environment("PKFS_SHOT") != "":
		await RenderingServer.frame_post_draw
		await RenderingServer.frame_post_draw
		get_viewport().get_texture().get_image().save_png(OS.get_environment("PKFS_SHOT"))
		get_tree().quit()

func _on_files_dropped(files: PackedStringArray) -> void:
	for f in files:
		if f.to_lower().ends_with(".nds"):
			_load_rom(f)
			return

func _load_rom(path: String) -> void:
	var rom := PkfsRom.new()
	if not rom.open(path):
		_label.text = "Could not open %s" % path.get_file()
		return
	if not rom.has_overworld():
		_label.text = "%s has no Gen 4 overworld (only DPPt/HGSS supported)" % rom.game_name()
		return

	var ow: Dictionary = rom.load_overworld()
	if ow.is_empty():
		_label.text = "%s: failed to load overworld" % rom.game_name()
		return

	if _map_root:
		_map_root.queue_free()
	_map_root = Node3D.new()
	add_child(_map_root)

	var w: int = ow["width"]
	var h: int = ow["height"]
	var land_ids: PackedInt32Array = ow["land_ids"]
	var altitudes: PackedInt32Array = ow["altitudes"]
	var ids: PackedInt32Array = ow["ids"]
	var glbs: Array = ow["glbs"]
	var glb_by_id := {}
	for i in ids.size():
		glb_by_id[ids[i]] = glbs[i]

	# Building models, keyed by the ids used in chunk_buildings placements.
	var building_keys: PackedInt32Array = ow.get("building_keys", PackedInt32Array())
	var building_glbs: Array = ow.get("building_glbs", [])
	var building_glb_by_key := {}
	for i in building_keys.size():
		building_glb_by_key[building_keys[i]] = building_glbs[i]
	var chunk_buildings: Dictionary = ow.get("chunk_buildings", {})
	var building_template_by_key := {}
	var building_count := 0

	var template_by_id := {}
	var placed_cells: Array[Vector2i] = []
	for y in h:
		for x in w:
			if placed_cells.size() >= max_cells:
				break
			var cell := y * w + x
			var lid: int = land_ids[cell]
			if lid == 65535 or not glb_by_id.has(lid):
				continue
			if not template_by_id.has(lid):
				template_by_id[lid] = _glb_to_node(glb_by_id[lid])
			var template = template_by_id[lid]
			if template == null:
				continue
			var alt := altitudes[cell] if cell < altitudes.size() else 0
			var origin := Vector3(x * CHUNK_SIZE, alt * altitude_unit, y * CHUNK_SIZE)
			var node: Node3D = template.duplicate()
			node.position = origin
			_map_root.add_child(node)
			placed_cells.append(Vector2i(x, y))
			building_count += _place_buildings(chunk_buildings.get(lid, []), origin, building_glb_by_key, building_template_by_key)

	print("pkfs: %s — %s %dx%d, placed %d chunks, %d buildings" % [rom.game_name(), ow["name"], w, h, placed_cells.size(), building_count])
	_label.text = "%s   [%s %dx%d]\nDrop another .nds to switch   RMB+WASD: fly" % [rom.game_name(), ow["name"], w, h]
	_focus_camera(placed_cells)

## Instantiate a chunk's buildings at their local transforms relative to the
## chunk origin. Returns the number placed.
func _place_buildings(list: Array, origin: Vector3, glb_by_key: Dictionary, template_by_key: Dictionary) -> int:
	var placed := 0
	for entry in list:
		var key: int = entry["key"]
		if not glb_by_key.has(key):
			continue
		if not template_by_key.has(key):
			template_by_key[key] = _glb_to_node(glb_by_key[key])
		var template = template_by_key[key]
		if template == null:
			continue
		var node: Node3D = template.duplicate()
		var pos: Vector3 = entry["pos"]
		node.position = origin + pos
		var scl: Vector3 = entry["scale"]
		if scl != Vector3.ZERO:
			node.scale = scl
		_map_root.add_child(node)
		placed += 1
	return placed

func _glb_to_node(bytes: PackedByteArray) -> Node3D:
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	if doc.append_from_buffer(bytes, "", state) != OK:
		return null
	return doc.generate_scene(state)

func _spawn_camera_rig() -> void:
	_cam = Camera3D.new()
	_cam.far = CHUNK_SIZE * 400.0
	add_child(_cam)
	_cam.position = Vector3(0, CHUNK_SIZE, CHUNK_SIZE)
	_cam.look_at(Vector3.ZERO)
	_move_speed = CHUNK_SIZE * 2.5

func _focus_camera(placed_cells: Array[Vector2i]) -> void:
	if placed_cells.is_empty():
		return
	var centroid := Vector2.ZERO
	for c in placed_cells:
		centroid += Vector2(c)
	centroid /= placed_cells.size()
	var best := placed_cells[0]
	var best_d := INF
	for c in placed_cells:
		var d := Vector2(c).distance_to(centroid)
		if d < best_d:
			best_d = d
			best = c
	var target := Vector3(best.x * CHUNK_SIZE, 0.0, best.y * CHUNK_SIZE)
	_cam.position = target + Vector3(0.0, CHUNK_SIZE * 0.7, CHUNK_SIZE * 1.0)
	_cam.look_at(target)
	_pitch = _cam.rotation.x
	_yaw = _cam.rotation.y

func _setup_environment() -> void:
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-55, -35, 0)
	add_child(light)
	var we := WorldEnvironment.new()
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.28, 0.36, 0.55)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(1, 1, 1)
	env.ambient_light_energy = 0.7
	we.environment = env
	add_child(we)

func _setup_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)
	_label = Label.new()
	_label.position = Vector2(12, 10)
	_label.add_theme_color_override("font_color", Color.WHITE)
	_label.add_theme_color_override("font_outline_color", Color.BLACK)
	_label.add_theme_constant_override("outline_size", 4)
	layer.add_child(_label)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_RIGHT:
			Input.mouse_mode = Input.MOUSE_MODE_CAPTURED if event.pressed else Input.MOUSE_MODE_VISIBLE
		elif event.button_index == MOUSE_BUTTON_WHEEL_UP:
			_move_speed *= 1.15
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			_move_speed /= 1.15
	elif event is InputEventMouseMotion and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED and _cam:
		_yaw -= event.relative.x * 0.004
		_pitch = clampf(_pitch - event.relative.y * 0.004, -1.5, 1.5)
		_cam.rotation = Vector3(_pitch, _yaw, 0.0)

func _process(delta: float) -> void:
	if _cam == null:
		return
	var dir := Vector3.ZERO
	var basis := _cam.global_transform.basis
	if Input.is_key_pressed(KEY_W): dir -= basis.z
	if Input.is_key_pressed(KEY_S): dir += basis.z
	if Input.is_key_pressed(KEY_A): dir -= basis.x
	if Input.is_key_pressed(KEY_D): dir += basis.x
	if Input.is_key_pressed(KEY_E): dir += Vector3.UP
	if Input.is_key_pressed(KEY_Q): dir += Vector3.DOWN
	if dir != Vector3.ZERO:
		var mult := 5.0 if Input.is_key_pressed(KEY_SHIFT) else 1.0
		_cam.position += dir.normalized() * _move_speed * mult * delta
