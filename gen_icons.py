from PIL import Image, ImageDraw

def create_pulse_icon(size):
    """Create a Pulse icon at the given size."""
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Outer blue circle
    margin = max(1, int(size * 0.05))
    draw.ellipse(
        [margin, margin, size - margin - 1, size - margin - 1],
        fill=(88, 166, 255, 255)
    )
    
    # Inner green circle (pulse dot)
    inner_margin = int(size * 0.28)
    draw.ellipse(
        [inner_margin, inner_margin, size - inner_margin - 1, size - inner_margin - 1],
        fill=(63, 185, 80, 255)
    )
    
    # Center highlight
    center_margin = int(size * 0.38)
    draw.ellipse(
        [center_margin, center_margin, size - center_margin - 1, size - center_margin - 1],
        fill=(100, 220, 120, 255)
    )
    
    return img

# Generate ICO file with multiple sizes
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
images = [create_pulse_icon(s) for s in ico_sizes]

icon_dir = r'C:\Users\33993\.qoderworkcn\workspace\mq9dca2eetej1k3e\pulse\src-tauri\icons'

# Save as ICO
images[0].save(
    f'{icon_dir}/icon.ico',
    format='ICO',
    sizes=[(s, s) for s in ico_sizes],
    append_images=images[1:]
)
print(f'Created icon.ico with sizes: {ico_sizes}')

# Also regenerate PNGs with better quality
for s in [32, 128, 256, 512]:
    img = create_pulse_icon(s)
    img.save(f'{icon_dir}/{s}x{s}.png' if s != 256 else f'{icon_dir}/128x128@2x.png')
    if s == 512:
        img.save(f'{icon_dir}/icon.png')
    print(f'Created {s}x{s}.png')

# Create ICNS placeholder (just copy the PNG)
import shutil
shutil.copy(f'{icon_dir}/icon.png', f'{icon_dir}/icon.icns')
print('Icon generation complete')
